//! Siembra idempotente de los usuarios de aplicación.
//!
//! Crea o actualiza `usuarios/usuario:{nombre}` con el hash Argon2id de la
//! contraseña. Las contraseñas **solo** llegan por variables de entorno y
//! nunca se imprimen ni se guardan en Git.
//!
//! Uso (lo envuelve `scripts/sembrar-usuarios.sh`):
//!
//! ```text
//! COUCHDB_URL=... BACKEND_USER=backend BACKEND_PASSWORD=... \
//! CAJERO_USUARIO=cajero1 CAJERO_PASSWORD=... \
//! ENCARGADO_USUARIO=encargado1 ENCARGADO_PASSWORD=... \
//! cargo run --bin sembrar_usuarios
//! ```

use std::{env, time::Duration};

use argon2::{Argon2, password_hash::PasswordHasher};
use chrono::{FixedOffset, SecondsFormat, Utc};
use reqwest::{Client, StatusCode, Url};
use serde_json::{Map, Value, json};

const ROLES: [(&str, &str, &str); 2] = [
    ("CAJERO_USUARIO", "cajero1", "cajero"),
    ("ENCARGADO_USUARIO", "encargado1", "encargado"),
];

#[tokio::main]
async fn main() {
    if let Err(error) = ejecutar().await {
        eprintln!("No se pudieron sembrar los usuarios: {error}");
        std::process::exit(1);
    }
}

async fn ejecutar() -> Result<(), String> {
    let url = requerida("COUCHDB_URL")?;
    let usuario_backend = env::var("BACKEND_USER").unwrap_or_else(|_| "backend".to_string());
    let contrasena_backend = requerida("BACKEND_PASSWORD")?;

    let base = Url::parse(&url).map_err(|e| format!("COUCHDB_URL inválida: {e}"))?;
    let cliente = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("No se pudo crear el cliente HTTP: {e}"))?;

    let mut nombres = Vec::new();

    for (variable, por_defecto, rol) in ROLES {
        let nombre = env::var(variable).unwrap_or_else(|_| por_defecto.to_string());
        let contrasena = requerida(&variable.replace("_USUARIO", "_PASSWORD"))?;

        validar_nombre(&nombre)?;
        if contrasena.len() < 8 {
            return Err(format!(
                "La contraseña de {variable} debe tener al menos 8 caracteres"
            ));
        }
        if nombres.contains(&nombre) {
            return Err(format!("El usuario '{nombre}' está repetido"));
        }
        nombres.push(nombre.clone());

        let hash = Argon2::default()
            .hash_password(contrasena.as_bytes())
            .map_err(|e| format!("No se pudo calcular el hash de {nombre}: {e}"))?
            .to_string();

        let zona = FixedOffset::west_opt(6 * 3600).expect("UTC-6 es válido");
        let creado_en = Utc::now()
            .with_timezone(&zona)
            .to_rfc3339_opts(SecondsFormat::Secs, false);

        let documento = json!({
            "tipo": "usuario",
            "nombre": nombre,
            "rol": rol,
            "hash_argon2": hash,
            "activo": true,
            "creado_en": creado_en,
        });

        guardar(
            &cliente,
            &base,
            &usuario_backend,
            &contrasena_backend,
            &nombre,
            documento,
        )
        .await?;
        println!("  usuario {nombre}: listo (rol {rol})");
    }

    Ok(())
}

/// Crea o actualiza el documento conservando los campos ajenos y la `_rev`.
async fn guardar(
    cliente: &Client,
    base: &Url,
    usuario: &str,
    contrasena: &str,
    nombre: &str,
    documento: Value,
) -> Result<(), String> {
    let id = format!("usuario:{nombre}");
    let url = documento_url(base, &id)?;

    let existente = cliente
        .get(url.clone())
        .basic_auth(usuario, Some(contrasena))
        .send()
        .await
        .map_err(|e| format!("No se pudo leer {id}: {e}"))?;

    let nuevo = match existente.status() {
        StatusCode::OK => {
            let mut actual: Map<String, Value> = existente
                .json()
                .await
                .map_err(|e| format!("Documento {id} inválido: {e}"))?;
            for (clave, valor) in documento.as_object().cloned().unwrap_or_default() {
                actual.insert(clave, valor);
            }
            Value::Object(actual)
        }
        StatusCode::NOT_FOUND => documento,
        otro => {
            return Err(format!(
                "CouchDB respondió {otro} al leer {id}; ¿ejecutaste infra/couchdb/setup.sh?"
            ));
        }
    };

    let respuesta = cliente
        .put(url)
        .basic_auth(usuario, Some(contrasena))
        .json(&nuevo)
        .send()
        .await
        .map_err(|e| format!("No se pudo guardar {id}: {e}"))?;

    match respuesta.status() {
        StatusCode::CREATED | StatusCode::ACCEPTED => Ok(()),
        otro => {
            let detalle = respuesta.text().await.unwrap_or_default();
            Err(format!(
                "CouchDB respondió {otro} al guardar {id}: {detalle}"
            ))
        }
    }
}

fn documento_url(base: &Url, id: &str) -> Result<Url, String> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|_| "COUCHDB_URL inválida".to_string())?
        .pop_if_empty()
        .extend(["usuarios", id]);
    Ok(url)
}

fn validar_nombre(nombre: &str) -> Result<(), String> {
    let valido = !nombre.is_empty()
        && nombre.len() <= 64
        && nombre
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');

    if valido {
        Ok(())
    } else {
        Err(format!(
            "El usuario '{nombre}' debe usar entre 1 y 64 caracteres: a-z, 0-9 o guion"
        ))
    }
}

fn requerida(nombre: &str) -> Result<String, String> {
    env::var(nombre).map_err(|_| format!("Falta {nombre} en el entorno"))
}
