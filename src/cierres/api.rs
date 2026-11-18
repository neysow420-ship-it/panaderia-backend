use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{FixedOffset, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::api::autenticacion::{Rol, Usuario};

use super::EstadoCierres;
use super::aplicacion::{ErrorCierre, ResultadoCierre, cerrar_dia, previsualizar_dia};
use super::dominio::{Cierre, ErrorCampo, SolicitudCierre};

/// `GET /cierre?fecha=AAAA-MM-DD`.
///
/// Devuelve el estado del día: si ya está cerrado, el documento oficial, y la
/// vista previa por producto (producido y vendido) para llenar el formulario.
pub async fn obtener_cierre(
    State(estado): State<Arc<EstadoCierres>>,
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
                vec![ErrorCampo::new(
                    "fecha",
                    "Indica la fecha con formato AAAA-MM-DD",
                )],
            );
        }
    };

    let preview = match previsualizar_dia(&estado.couch, filtro.fecha).await {
        Ok(preview) => preview,
        Err(ErrorCierre::Validacion(errores)) => {
            return error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores);
        }
        Err(fallo) => {
            eprintln!("GET /cierre: {fallo:?}");
            return error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![]);
        }
    };

    let cierre: Option<Cierre> = if preview.cerrado {
        match estado.couch.leer_cierre(filtro.fecha).await {
            Ok(cierre) => cierre,
            Err(detalle) => {
                eprintln!("GET /cierre: no se pudo leer el cierre: {detalle:?}");
                return error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![]);
            }
        }
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(json!({
            "fecha": preview.fecha,
            "cerrado": preview.cerrado,
            "cierre": cierre,
            "por_producto": preview.por_producto,
        })),
    )
        .into_response()
}

/// Filtro `?fecha=AAAA-MM-DD`.
#[derive(Deserialize)]
pub struct FiltroFecha {
    pub fecha: NaiveDate,
}

/// POST /cierre. Solo el encargado puede cerrar el día.
pub async fn crear_cierre(
    State(estado): State<Arc<EstadoCierres>>,
    Extension(usuario): Extension<Usuario>,
    cuerpo: Result<Json<SolicitudCierre>, JsonRejection>,
) -> Response {
    // 1. Verificar permisos.
    if let Some(respuesta) = exigir_encargado(&usuario) {
        return respuesta;
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

/// Devuelve `403` si el usuario no es encargado.
fn exigir_encargado(usuario: &Usuario) -> Option<Response> {
    match usuario.rol {
        Rol::Encargado => None,
        Rol::Cajero => Some(error(
            StatusCode::FORBIDDEN,
            "prohibido",
            vec![ErrorCampo::new(
                "rol",
                "Solo el encargado puede cerrar o consultar el día",
            )],
        )),
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
