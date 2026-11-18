//! Controlador web de `GET /vencimientos`.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::EstadoVencimientos;
use crate::api::autenticacion::{Rol, Usuario};

/// Fecha opcional de consulta (`AAAA-MM-DD`). Por defecto, hoy en El Salvador.
#[derive(Deserialize)]
pub struct FiltroFecha {
    pub fecha: Option<String>,
}

/// Handler HTTP que arma el reporte de vencimientos desde CouchDB.
pub async fn obtener_vencimientos_handler(
    State(estado): State<Arc<EstadoVencimientos>>,
    Extension(usuario): Extension<Usuario>,
    Query(filtro): Query<FiltroFecha>,
) -> Response {
    // Solo el encargado consulta producción y vencimientos.
    if let Rol::Cajero = usuario.rol {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "codigo": "prohibido",
                "errores": [{
                    "campo": "rol",
                    "motivo": "Solo el encargado puede consultar los vencimientos"
                }]
            })),
        )
            .into_response();
    }

    match estado.caso.generar_reporte_vencimientos(filtro.fecha).await {
        Ok(reporte) => (StatusCode::OK, Json(reporte)).into_response(),
        Err(detalle) => {
            eprintln!("GET /vencimientos: base no disponible: {detalle}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "codigo": "no_disponible",
                    "errores": []
                })),
            )
                .into_response()
        }
    }
}
