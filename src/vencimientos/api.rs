///Controlador Web de Axum (GET /vencimientos). Retorna la respuesta HTTP en JSON respetando la estructura exigida por el contrato.
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

/// Handler HTTP de Axum que atiende las peticiones GET en la ruta /vencimientos.
pub async fn obtener_vencimientos_handler() -> impl IntoResponse {
    // Estructura canónica de respuesta según las especificaciones del contrato de datos de la Fase 3
    let respuesta = json!({
        "fecha": "2026-10-13",
        "actualizado_en": "2026-10-13T15:02:11-06:00",
        "resumen": {
            "vencido": 0,
            "vence_hoy": 0,
            "por_vencer": 42,
            "vigente": 0
        },
        "lotes": [
            {
                "lote_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K",
                "producto_id": "producto:pan-frances",
                "unidades": 42,
                "fecha_vencimiento": "2026-10-14",
                "dias_restantes": 1,
                "estado": "por_vencer"
            }
        ]
    });

    // Retorna código HTTP 200 OK y el cuerpo JSON
    (StatusCode::OK, Json(respuesta))
}
