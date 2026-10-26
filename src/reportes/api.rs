use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State, rejection::QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::EstadoReportes;
use super::aplicacion::{ErrorReporte, reporte_merma, reporte_ventas};
use super::dominio::{ErrorCampo, ParametrosRango};
use crate::api::autenticacion::{Rol, Usuario};
use chrono::{FixedOffset, Utc};

/// GET /reportes/ventas
/// Devuelve las ventas agrupadas por día y producto.
pub async fn obtener_reporte_ventas(
    State(estado): State<Arc<EstadoReportes>>,
    Extension(usuario): Extension<Usuario>,
    parametros: Result<Query<ParametrosRango>, QueryRejection>,
) -> Response {
    // Solo el encargado puede consultar los reportes.
    match usuario.rol {
        Rol::Encargado => {}
        Rol::Cajero => {
            return error(
                StatusCode::FORBIDDEN,
                "prohibido",
                vec![ErrorCampo::new(
                    "rol",
                    "Solo el encargado puede consultar reportes",
                )],
            );
        }
    }

    // Interpretar los parámetros ?desde=AAAA-MM-DD&hasta=AAAA-MM-DD.
    let Query(rango) = match parametros {
        Ok(parametros) => parametros,
        Err(_) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "solicitud_invalida",
                vec![ErrorCampo::new(
                    "rango",
                    "Indica desde y hasta con formato AAAA-MM-DD",
                )],
            );
        }
    };

    match reporte_ventas(&estado.couch, &rango).await {
        Ok(reporte) => (StatusCode::OK, Json(reporte)).into_response(),
        Err(ErrorReporte::Validacion(errores)) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores)
        }
        Err(ErrorReporte::NoDisponible(detalle)) => {
            eprintln!("GET /reportes/ventas: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}

fn error(status: StatusCode, codigo: &str, errores: Vec<ErrorCampo>) -> Response {
    (
        status,
        Json(json!({
            "codigo": codigo,
            "errores": errores
        })),
    )
        .into_response()
}

/// GET /merma
/// Consulta cierres históricos y muestra una vista previa del día actual.
pub async fn obtener_reporte_merma(
    State(estado): State<Arc<EstadoReportes>>,
    Extension(usuario): Extension<Usuario>,
    parametros: Result<Query<ParametrosRango>, QueryRejection>,
) -> Response {
    // Los reportes solo están disponibles para el encargado.
    match usuario.rol {
        Rol::Encargado => {}
        Rol::Cajero => {
            return error(
                StatusCode::FORBIDDEN,
                "prohibido",
                vec![ErrorCampo::new(
                    "rol",
                    "Solo el encargado puede consultar reportes",
                )],
            );
        }
    }

    let Query(rango) = match parametros {
        Ok(parametros) => parametros,
        Err(_) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "solicitud_invalida",
                vec![ErrorCampo::new(
                    "rango",
                    "Indica desde y hasta con formato AAAA-MM-DD",
                )],
            );
        }
    };

    // El día de negocio se determina con la zona horaria de El Salvador.
    let zona = FixedOffset::west_opt(6 * 3600).expect("El desfase horario es válido");
    let ahora = Utc::now().with_timezone(&zona);

    match reporte_merma(
        &estado.couch,
        &rango,
        ahora.date_naive(),
        ahora.to_rfc3339(),
    )
    .await
    {
        Ok(reporte) => (StatusCode::OK, Json(reporte)).into_response(),

        Err(ErrorReporte::Validacion(errores)) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores)
        }

        Err(ErrorReporte::NoDisponible(detalle)) => {
            eprintln!("GET /merma: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}
