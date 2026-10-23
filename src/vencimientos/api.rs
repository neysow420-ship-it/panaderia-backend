//! Controlador web de `GET /vencimientos`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::EstadoVencimientos;

/// Fecha opcional de consulta (`AAAA-MM-DD`). Por defecto, hoy en El Salvador.
#[derive(Deserialize)]
pub struct FiltroFecha {
    pub fecha: Option<String>,
}

/// Handler HTTP que arma el reporte de vencimientos desde CouchDB.
pub async fn obtener_vencimientos_handler(
    State(estado): State<Arc<EstadoVencimientos>>,
    Query(filtro): Query<FiltroFecha>,
) -> Response {
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
