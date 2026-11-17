//! # Rutas HTTP de descartes
//!
//! | Ruta                          | Método | Qué hace |
//! |-------------------------------|--------|----------|
//! | `/descartes?fecha=AAAA-MM-DD` | GET    | Lista los descartes del día |
//! | `/descartes`                  | POST   | Registra un descarte |
//!
//! Ambas están protegidas: el middleware de `src/api/autenticacion.rs` deja un
//! [`Usuario`] en la petición y aquí se comprueba que sea `encargado`. El nombre
//! del usuario **nunca** se toma del cuerpo: lo pone el servidor desde el JWT.

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::NaiveDate;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::api::autenticacion::{Rol, Usuario};
use crate::descartes::{
    aplicacion::{self, ErrorDescarte},
    dominio::Descarte,
};

/// Estructura para recibir el JSON desde el cliente (frontend).
///
/// No incluye `usuario`: se toma del token para que nadie pueda atribuir un
/// descarte a otra persona.
#[derive(Deserialize)]
pub struct CrearDescarteDto {
    pub fecha: String,
    pub lote_id: String,
    pub producto_id: String,
    pub unidades: i32,
    pub motivo: String,
    pub creado_en: String,
}

/// Filtro `?fecha=AAAA-MM-DD` de las lecturas.
#[derive(Deserialize)]
pub struct FiltroFecha {
    pub fecha: NaiveDate,
}

/// Estado compartido que se inyecta en las rutas (CouchDB y credenciales).
#[derive(Clone)]
pub struct AppState {
    pub couchdb_url: String,
    pub couchdb_user: String,
    pub couchdb_password: String,
    pub http_client: Client,
}

/// `POST /descartes`: registra un descarte del encargado.
pub async fn post_descarte(
    State(state): State<AppState>,
    Extension(usuario): Extension<Usuario>,
    Json(payload): Json<CrearDescarteDto>,
) -> Response {
    if let Some(respuesta) = exigir_encargado(&usuario) {
        return respuesta;
    }

    let nuevo_descarte = Descarte {
        _id: String::new(), // Se genera en la capa de aplicación.
        tipo: "descarte".to_string(),
        fecha: payload.fecha,
        lote_id: payload.lote_id,
        producto_id: payload.producto_id,
        unidades: payload.unidades,
        motivo: payload.motivo,
        // El usuario sale del JWT, no del cuerpo de la petición.
        usuario: usuario.nombre,
        creado_en: payload.creado_en,
    };

    match aplicacion::registrar_descarte(
        &state.http_client,
        &state.couchdb_url,
        &state.couchdb_user,
        &state.couchdb_password,
        nuevo_descarte,
    )
    .await
    {
        Ok(descarte_guardado) => (StatusCode::CREATED, Json(descarte_guardado)).into_response(),
        Err(ErrorDescarte::Validacion(motivo)) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validacion",
            vec![json!({ "campo": "descarte", "motivo": motivo })],
        ),
        Err(ErrorDescarte::NoDisponible(detalle)) => {
            eprintln!("POST /descartes: base no disponible: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}

/// `GET /descartes?fecha=AAAA-MM-DD`: lista los descartes del día.
pub async fn listar_descartes(
    State(state): State<AppState>,
    Extension(usuario): Extension<Usuario>,
    filtro: Result<Query<FiltroFecha>, axum::extract::rejection::QueryRejection>,
) -> Response {
    if let Some(respuesta) = exigir_encargado(&usuario) {
        return respuesta;
    }

    let Query(filtro) = match filtro {
        Ok(filtro) => filtro,
        Err(_) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "solicitud_invalida",
                vec![json!({
                    "campo": "fecha",
                    "motivo": "Indica la fecha con formato AAAA-MM-DD"
                })],
            );
        }
    };

    match aplicacion::listar_descartes(
        &state.http_client,
        &state.couchdb_url,
        &state.couchdb_user,
        &state.couchdb_password,
        &filtro.fecha.to_string(),
    )
    .await
    {
        Ok(descartes) => (StatusCode::OK, Json(json!({ "descartes": descartes }))).into_response(),
        Err(ErrorDescarte::NoDisponible(detalle)) => {
            eprintln!("GET /descartes: base no disponible: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
        Err(ErrorDescarte::Validacion(motivo)) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validacion",
            vec![json!({ "campo": "fecha", "motivo": motivo })],
        ),
    }
}

/// Devuelve `403` si el usuario no es encargado.
fn exigir_encargado(usuario: &Usuario) -> Option<Response> {
    match usuario.rol {
        Rol::Encargado => None,
        Rol::Cajero => Some(error(
            StatusCode::FORBIDDEN,
            "prohibido",
            vec![json!({
                "campo": "rol",
                "motivo": "Solo el encargado puede registrar o consultar descartes"
            })],
        )),
    }
}

/// Formato uniforme de errores de la API.
fn error(codigo: StatusCode, nombre: &str, errores: Vec<serde_json::Value>) -> Response {
    (
        codigo,
        Json(json!({ "codigo": nombre, "errores": errores })),
    )
        .into_response()
}
