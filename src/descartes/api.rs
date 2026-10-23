use axum::{Json, extract::State, http::StatusCode};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::descartes::{
    aplicacion::{self, ErrorDescarte},
    dominio::Descarte,
};

/// Estructura para recibir el JSON desde el cliente (frontend).
#[derive(Deserialize)]
pub struct CrearDescarteDto {
    pub fecha: String,
    pub lote_id: String,
    pub producto_id: String,
    pub unidades: i32,
    pub motivo: String,
    pub usuario: String,
    pub creado_en: String,
}

/// Estado compartido que se inyecta en las rutas (CouchDB y credenciales).
#[derive(Clone)]
pub struct AppState {
    pub couchdb_url: String,
    pub couchdb_user: String,
    pub couchdb_password: String,
    pub http_client: Client,
}

/// Endpoint HTTP: `POST /descartes`.
pub async fn post_descarte(
    State(state): State<AppState>,
    Json(payload): Json<CrearDescarteDto>,
) -> Result<(StatusCode, Json<Descarte>), (StatusCode, Json<serde_json::Value>)> {
    // Mapeamos el DTO recibido hacia la entidad del dominio.
    let nuevo_descarte = Descarte {
        _id: String::new(), // Se genera en la capa de aplicación.
        tipo: "descarte".to_string(),
        fecha: payload.fecha,
        lote_id: payload.lote_id,
        producto_id: payload.producto_id,
        unidades: payload.unidades,
        motivo: payload.motivo,
        usuario: payload.usuario,
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
        Ok(descarte_guardado) => Ok((StatusCode::CREATED, Json(descarte_guardado))),
        Err(ErrorDescarte::Validacion(motivo)) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "codigo": "validacion",
                "errores": [{ "campo": "descarte", "motivo": motivo }]
            })),
        )),
        Err(ErrorDescarte::NoDisponible(detalle)) => {
            eprintln!("POST /descartes: base no disponible: {detalle}");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "codigo": "no_disponible", "errores": [] })),
            ))
        }
    }
}
