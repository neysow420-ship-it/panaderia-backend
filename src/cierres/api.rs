use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{FixedOffset, Utc};
use serde_json::json;

use crate::api::autenticacion::{Rol, Usuario};

use super::EstadoCierres;
use super::aplicacion::{ErrorCierre, ResultadoCierre, cerrar_dia};
use super::dominio::{Cierre, ErrorCampo, SolicitudCierre};

/// POST /cierre. Solo el encargado puede cerrar el día.
pub async fn crear_cierre(
    State(estado): State<Arc<EstadoCierres>>,
    Extension(usuario): Extension<Usuario>,
    cuerpo: Result<Json<SolicitudCierre>, JsonRejection>,
) -> Response {
    // 1. Verificar permisos.
    match usuario.rol {
        Rol::Encargado => {}
        Rol::Cajero => {
            return error(
                StatusCode::FORBIDDEN,
                "prohibido",
                vec![ErrorCampo::new(
                    "rol",
                    "Solo el encargado puede cerrar el día",
                )],
            );
        }
    }

    // 2. Interpretar el JSON.
    let Json(solicitud) = match cuerpo {
        Ok(cuerpo) => cuerpo,
        Err(rechazo) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "solicitud_invalida",
                vec![ErrorCampo::new("cuerpo", &rechazo.body_text())],
            );
        }
    };

    // 3. Usar la hora del servidor en la zona de El Salvador (UTC-6).
    let zona = FixedOffset::west_opt(6 * 3600).expect("El desfase horario es válido");
    let ahora = Utc::now().with_timezone(&zona);

    // 4. Ejecutar el caso de uso.
    let resultado = cerrar_dia(&estado.couch, &solicitud, &usuario.nombre, ahora).await;

    // 5. Traducir el resultado al contrato HTTP.
    match resultado {
        Ok(ResultadoCierre::Creado(cierre)) => exito(StatusCode::CREATED, cierre, false),
        Ok(ResultadoCierre::YaProcesado(cierre)) => exito(StatusCode::OK, cierre, true),
        Err(ErrorCierre::Validacion(errores)) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores)
        }
        Err(ErrorCierre::DiferenciaNegativa(errores)) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "diferencia_negativa",
            errores,
        ),
        Err(ErrorCierre::ConflictoContenido) => error(
            StatusCode::CONFLICT,
            "cierre_en_conflicto",
            vec![ErrorCampo::new(
                "fecha",
                "Ese día ya tiene un cierre con otro contenido",
            )],
        ),
        Err(ErrorCierre::NoDisponible(detalle)) => {
            eprintln!("POST /cierre: dependencia no disponible: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}

/// Respuesta definida en el contrato: { "ya_procesada": bool, "cierre": {...} }.
fn exito(codigo: StatusCode, cierre: Cierre, ya_procesada: bool) -> Response {
    (
        codigo,
        Json(json!({
            "ya_procesada": ya_procesada,
            "cierre": cierre
        })),
    )
        .into_response()
}

/// Formato uniforme de errores de la API.
fn error(codigo: StatusCode, nombre: &str, errores: Vec<ErrorCampo>) -> Response {
    (
        codigo,
        Json(json!({
            "codigo": nombre,
            "errores": errores
        })),
    )
        .into_response()
}
