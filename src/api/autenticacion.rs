use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    extract::{Request, State, rejection::JsonRejection},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Estado;
use crate::config::ModoAuth;

/// Roles admitidos por el contrato.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rol {
    Cajero,
    Encargado,
}

/// Identidad que el middleware deja disponible para las rutas.
#[derive(Clone, Debug)]
pub struct Usuario {
    pub nombre: String,
    pub rol: Rol,
}

#[derive(Deserialize)]
pub(crate) struct SolicitudLogin {
    usuario: String,
    contrasena: String,
}

#[derive(Deserialize)]
struct UsuarioDocumento {
    #[serde(rename = "_id")]
    id: String,
    tipo: String,
    nombre: String,
    rol: String,
    hash_argon2: String,
    activo: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    rol: Rol,
    exp: usize,
}

#[derive(Serialize)]
struct UsuarioSesion {
    nombre: String,
    rol: Rol,
}

#[derive(Serialize)]
struct RespuestaLogin {
    token: String,
    tipo: &'static str,
    expira_en: String,
    usuario: UsuarioSesion,
}

/// POST /auth/login
pub(crate) async fn iniciar_sesion(
    State(estado): State<Arc<Estado>>,
    cuerpo: Result<Json<SolicitudLogin>, JsonRejection>,
) -> Response {
    let Json(solicitud) = match cuerpo {
        Ok(cuerpo) => cuerpo,
        Err(_) => {
            return error_con_campo(
                StatusCode::UNPROCESSABLE_ENTITY,
                "solicitud_invalida",
                "cuerpo",
                "Se requiere un JSON válido con usuario y contrasena",
            );
        }
    };

    // Validar el nombre antes de usarlo para consultar CouchDB.
    let nombre = solicitud.usuario.as_str();
    let nombre_valido = !nombre.is_empty()
        && nombre.len() <= 64
        && nombre
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');

    if !nombre_valido {
        return error_con_campo(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validacion",
            "usuario",
            "Debe contener entre 1 y 64 caracteres: a-z, 0-9 o guion",
        );
    }

    // El backend consulta CouchDB usando sus credenciales técnicas.
    let url = format!(
        "{}/usuarios/usuario:{}",
        estado.config.couchdb_url.trim_end_matches('/'),
        nombre
    );

    let respuesta = match estado
        .cliente
        .get(&url)
        .basic_auth(
            &estado.config.couchdb_user,
            Some(&estado.config.couchdb_password),
        )
        .send()
        .await
    {
        Ok(respuesta) => respuesta,
        Err(error) => {
            eprintln!("POST /auth/login: error de conexión con CouchDB: {error}");
            return error_sin_campos(StatusCode::SERVICE_UNAVAILABLE, "no_disponible");
        }
    };

    if respuesta.status().as_u16() == 404 {
        return credenciales_invalidas();
    }

    if !respuesta.status().is_success() {
        eprintln!("POST /auth/login: CouchDB respondió {}", respuesta.status());
        return error_sin_campos(StatusCode::SERVICE_UNAVAILABLE, "no_disponible");
    }

    let documento: UsuarioDocumento = match respuesta.json().await {
        Ok(documento) => documento,
        Err(error) => {
            eprintln!("POST /auth/login: documento de usuario inválido: {error}");
            return error_sin_campos(StatusCode::SERVICE_UNAVAILABLE, "no_disponible");
        }
    };

    if documento.id != format!("usuario:{nombre}")
        || documento.tipo != "usuario"
        || documento.nombre != nombre
    {
        eprintln!("POST /auth/login: documento inconsistente");
        return error_sin_campos(StatusCode::SERVICE_UNAVAILABLE, "no_disponible");
    }

    if !documento.activo {
        return credenciales_invalidas();
    }

    let rol = match documento.rol.as_str() {
        "cajero" => Rol::Cajero,
        "encargado" => Rol::Encargado,
        _ => {
            eprintln!("POST /auth/login: el usuario tiene un rol inválido");
            return error_sin_campos(StatusCode::SERVICE_UNAVAILABLE, "no_disponible");
        }
    };

    // Solo aceptamos hashes Argon2id, nunca contraseñas en texto plano.
    let hash = match PasswordHash::new(&documento.hash_argon2) {
        Ok(hash) if hash.algorithm.as_str() == "argon2id" => hash,
        _ => {
            eprintln!("POST /auth/login: hash Argon2id inválido");
            return error_sin_campos(StatusCode::SERVICE_UNAVAILABLE, "no_disponible");
        }
    };

    if Argon2::default()
        .verify_password(solicitud.contrasena.as_bytes(), &hash)
        .is_err()
    {
        return credenciales_invalidas();
    }

    // El JWT vence doce horas después de iniciar sesión (contrato de la fase 4B).
    let expira_en = Utc::now() + Duration::hours(12);
    let claims = Claims {
        sub: documento.nombre.clone(),
        rol,
        exp: expira_en.timestamp() as usize,
    };

    let token = match encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(estado.config.jwt_secret.as_bytes()),
    ) {
        Ok(token) => token,
        Err(error) => {
            eprintln!("POST /auth/login: no se pudo emitir el JWT: {error}");
            return error_sin_campos(StatusCode::INTERNAL_SERVER_ERROR, "error_interno");
        }
    };

    (
        StatusCode::OK,
        Json(RespuestaLogin {
            token,
            tipo: "Bearer",
            expira_en: expira_en.to_rfc3339(),
            usuario: UsuarioSesion {
                nombre: documento.nombre,
                rol,
            },
        }),
    )
        .into_response()
}

/// Middleware de autenticación para las rutas protegidas.
pub(crate) async fn autenticar(
    State(estado): State<Arc<Estado>>,
    mut peticion: Request,
    siguiente: Next,
) -> Response {
    // Modo de desarrollo que ya existía: permite probar sin login real.
    if estado.config.auth_mode == ModoAuth::Desactivado {
        peticion.extensions_mut().insert(Usuario {
            nombre: "encargado".to_string(),
            rol: Rol::Encargado,
        });

        return siguiente.run(peticion).await;
    }

    let valor = match peticion
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        Some(valor) => valor,
        None => return no_autorizado(),
    };

    let (esquema, token) = match valor.split_once(' ') {
        Some(partes) => partes,
        None => return no_autorizado(),
    };

    if !esquema.eq_ignore_ascii_case("Bearer") || token.trim().is_empty() {
        return no_autorizado();
    }

    let validacion = Validation::new(Algorithm::HS256);

    let claims = match decode::<Claims>(
        token.trim(),
        &DecodingKey::from_secret(estado.config.jwt_secret.as_bytes()),
        &validacion,
    ) {
        Ok(datos) => datos.claims,
        Err(_) => return no_autorizado(),
    };

    peticion.extensions_mut().insert(Usuario {
        nombre: claims.sub,
        rol: claims.rol,
    });

    siguiente.run(peticion).await
}

fn credenciales_invalidas() -> Response {
    error_con_campo(
        StatusCode::UNAUTHORIZED,
        "credenciales_invalidas",
        "usuario",
        "Usuario o contraseña incorrectos",
    )
}

fn no_autorizado() -> Response {
    error_con_campo(
        StatusCode::UNAUTHORIZED,
        "no_autorizado",
        "Authorization",
        "Falta un token Bearer válido o ha expirado",
    )
}

fn error_con_campo(status: StatusCode, codigo: &str, campo: &str, motivo: &str) -> Response {
    (
        status,
        Json(json!({
            "codigo": codigo,
            "errores": [{ "campo": campo, "motivo": motivo }]
        })),
    )
        .into_response()
}

fn error_sin_campos(status: StatusCode, codigo: &str) -> Response {
    (
        status,
        Json(json!({
            "codigo": codigo,
            "errores": []
        })),
    )
        .into_response()
}
