//! # Ruta HTTP `POST /ventas`
//!
//! Es la "puerta de entrada": recibe el JSON del cliente, llama al caso de uso
//! [`registrar_venta`] y convierte el resultado en una respuesta HTTP.
//!
//! | Situación                               | Código | Cuerpo |
//! |-----------------------------------------|--------|--------|
//! | Venta nueva guardada                    | 201    | `{ ya_procesada: false, venta }` |
//! | Reenvío con mismo ID y mismo contenido  | 200    | `{ ya_procesada: true, venta }` |
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

use super::EstadoVentas;
use super::aplicacion::{ErrorVenta, ResultadoVenta, registrar_venta};
use super::dominio::{ErrorCampo, SolicitudVenta, Venta, zona_negocio};
// Importado de la parte de Login (no se modifica, solo se usa).
use crate::api::autenticacion::Usuario;

/// Handler de `POST /ventas`.
///
/// Axum llena los parámetros automáticamente:
/// - `State`: el estado de ventas (CouchDB y catálogo), ver `mod.rs`.
/// - `Extension<Usuario>`: el usuario que dejó el middleware de autenticación.
/// - `cuerpo`: el JSON ya convertido a [`SolicitudVenta`]. Se recibe como `Result`
///   para responder con nuestro formato de error si el JSON viene mal, en vez
///   del mensaje por defecto de Axum.
pub async fn crear_venta(
    State(estado): State<Arc<EstadoVentas>>,
    Extension(usuario): Extension<Usuario>,
    cuerpo: Result<Json<SolicitudVenta>, JsonRejection>,
) -> Response {
    // 1. ¿El JSON tiene la forma correcta? Si no, 422 con el motivo.
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

    // 2. Nombre del usuario para el campo `usuario` de la venta.
    //    Hoy el `Usuario` del middleware solo trae el rol, así que se guarda el rol
    //    como texto ("encargado"). Cuando el login agregue el nombre, se cambia
    //    esta parte por `usuario.nombre`.
    let nombre_usuario = serde_json::to_value(usuario.rol)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "desconocido".to_string());

    // 3. Hora actual en El Salvador y ejecución del caso de uso.
    let ahora = Utc::now().with_timezone(&zona_negocio());
    let resultado = registrar_venta(
        &estado.couch,
        &estado.catalogo,
        &solicitud,
        &nombre_usuario,
        ahora,
    )
    .await;

    // 4. Traducir el resultado a código HTTP y JSON.
    match resultado {
        Ok(ResultadoVenta::Creada(v)) => exito(StatusCode::CREATED, v, false),
        Ok(ResultadoVenta::YaProcesada(v)) => exito(StatusCode::OK, v, true),
        Err(ErrorVenta::Validacion(errores)) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores)
        }
        Err(ErrorVenta::ConflictoOperacion) => error(
            StatusCode::CONFLICT,
            "operacion_en_conflicto",
            vec![ErrorCampo::new(
                "operacion_id",
                "ya existe una venta con este ID y otro contenido; corrija con un ULID nuevo",
            )],
        ),
        Err(ErrorVenta::NoDisponible(detalle)) => {
            // El detalle técnico va al log del servidor, no al cliente.
            eprintln!("POST /ventas: base no disponible: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}

/// Respuesta de éxito: `{ "ya_procesada": bool, "venta": {...} }`.
/// Se quita `_rev` porque es un dato interno de CouchDB que el cliente no necesita.
fn exito(codigo: StatusCode, mut venta: Venta, ya_procesada: bool) -> Response {
    venta.rev = None;
    (
        codigo,
        Json(json!({ "ya_procesada": ya_procesada, "venta": venta })),
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
