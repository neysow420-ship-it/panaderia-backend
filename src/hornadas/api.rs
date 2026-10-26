//! # Ruta HTTP `POST /hornadas`
//!
//! Es la "puerta de entrada": recibe el JSON del cliente, llama al caso de uso
//! [`registrar_hornada`] y convierte el resultado en una respuesta HTTP.
//!
//! | Situación                               | Código | Cuerpo |
//! |-----------------------------------------|--------|--------|
//! | Hornada nueva guardada                  | 201    | `{ ya_procesada: false, hornada }` |
//! | Reenvío con mismo ID y mismo contenido  | 200    | `{ ya_procesada: true, hornada }` |
//! | Mismo ID con contenido distinto         | 409    | `{ codigo, errores }` |
//! | JSON mal formado o datos inválidos      | 422    | `{ codigo, errores }` |
//! | CouchDB no disponible                   | 503    | `{ codigo, errores: [] }` |
//!
//! Relación con **Login**: la ruta está dentro de las rutas protegidas de
//! `main.rs`, así que antes de llegar aquí pasa por el middleware de
//! `src/api/autenticacion.rs`, que deja un [`Usuario`] en la petición.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde_json::json;

use super::EstadoHornadas;
use super::aplicacion::{ErrorHornada, ResultadoHornada, registrar_hornada};
use super::dominio::{ErrorCampo, Hornada, SolicitudHornada, zona_negocio};
// Importado de la parte de Login (no se modifica, solo se usa).
use crate::api::autenticacion::{Rol, Usuario};

/// Handler de `POST /hornadas`.
///
/// Axum llena los parámetros automáticamente:
/// - `State`: el estado de hornadas (CouchDB), ver `mod.rs`.
/// - `Extension<Usuario>`: el usuario que dejó el middleware de autenticación.
/// - `cuerpo`: el JSON ya convertido a [`SolicitudHornada`]. Se recibe como `Result`
///   para responder con nuestro formato de error si el JSON viene mal (o trae campos
///   que el contrato no permite), en vez del mensaje por defecto de Axum.
pub async fn crear_hornada(
    State(estado): State<Arc<EstadoHornadas>>,
    Extension(usuario): Extension<Usuario>,
    cuerpo: Result<Json<SolicitudHornada>, JsonRejection>,
) -> Response {
    // 1. Permisos: según el contrato (§9) solo el `encargado` registra hornadas.
    //    Hoy `Rol` solo tiene `Encargado`, así que siempre pasa. El `match` es
    //    exhaustivo a propósito: cuando el login agregue `Cajero`, este archivo
    //    dejará de compilar hasta que se decida responder `403 prohibido`.
    match usuario.rol {
        Rol::Encargado => {}
        Rol::Cajero => {
            return error(
                StatusCode::FORBIDDEN,
                "prohibido",
                vec![ErrorCampo::new(
                    "rol",
                    "Solo el encargado puede registrar hornadas",
                )],
            );
        }
    }

    // 2. ¿El JSON tiene la forma correcta? Si no, 422 con el motivo.
    let Json(solicitud) = match cuerpo {
        Ok(c) => c,
        Err(rechazo) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "solicitud_invalida",
                vec![ErrorCampo::new("cuerpo", &rechazo.body_text())],
            );
        }
    };

    // 3. Nombre del usuario que registra la hornada.
    let nombre_usuario = usuario.nombre;
    // 4. Hora actual en El Salvador y ejecución del caso de uso.
    let ahora = Utc::now().with_timezone(&zona_negocio());
    let resultado = registrar_hornada(&estado.couch, &solicitud, &nombre_usuario, ahora).await;

    // 5. Traducir el resultado a código HTTP y JSON.
    match resultado {
        Ok(ResultadoHornada::Creada(h)) => exito(StatusCode::CREATED, h, false),
        Ok(ResultadoHornada::YaProcesada(h)) => exito(StatusCode::OK, h, true),
        Err(ErrorHornada::Validacion(errores)) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores)
        }
        Err(ErrorHornada::ConflictoOperacion) => error(
            StatusCode::CONFLICT,
            "operacion_en_conflicto",
            vec![ErrorCampo::new(
                "operacion_id",
                "ya existe una hornada con este ID y otro contenido; corrija con un ULID nuevo",
            )],
        ),
        Err(ErrorHornada::NoDisponible(detalle)) => {
            // El detalle técnico va al log del servidor, no al cliente.
            eprintln!("POST /hornadas: base no disponible: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}

/// Respuesta de éxito: `{ "ya_procesada": bool, "hornada": {...} }`.
/// Se quita `_rev` porque es un dato interno de CouchDB que el cliente no necesita.
fn exito(codigo: StatusCode, mut hornada: Hornada, ya_procesada: bool) -> Response {
    hornada.rev = None;
    (
        codigo,
        Json(json!({ "ya_procesada": ya_procesada, "hornada": hornada })),
    )
        .into_response()
}

/// Respuesta de error con la forma del contrato:
/// `{ "codigo": "...", "errores": [ { "campo": "...", "motivo": "..." } ] }`.
fn error(codigo: StatusCode, nombre: &str, errores: Vec<ErrorCampo>) -> Response {
    (
        codigo,
        Json(json!({ "codigo": nombre, "errores": errores })),
    )
        .into_response()
}
