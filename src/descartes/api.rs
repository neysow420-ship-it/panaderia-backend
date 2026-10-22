use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::descartes::{dominio::Descarte, aplicacion};

/// Estructura para recibir el JSON desde el cliente (frontend)
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

/// Estado compartido que se inyecta en las rutas (URL de CouchDB y cliente HTTP)
#[derive(Clone)]
pub struct AppState {
    pub couchdb_url: String,
    pub http_client: Client,
}

/// Endpoint HTTP: POST /descartes
pub async fn post_descarte(
    State(state): State<AppState>,
    Json(payload): Json<CrearDescarteDto>,
) -> Result<(StatusCode, Json<Descarte>), (StatusCode, String)> {
    
    // Mapeamos el DTO recibido hacia la entidad del Dominio
    let nuevo_descarte = Descarte {
        _id: String::new(), // Se generará en la capa de aplicación
        tipo: "descarte".to_string(),
        fecha: payload.fecha,
        lote_id: payload.lote_id,
        producto_id: payload.producto_id,
        unidades: payload.unidades,
        motivo: payload.motivo,
        usuario: payload.usuario,
        creado_en: payload.creado_en,
    };

    // Ejecutamos el caso de uso
    match aplicacion::registrar_descarte(&state.http_client, &state.couchdb_url, nuevo_descarte).await {
        Ok(descarte_guardado) => Ok((StatusCode::CREATED, Json(descarte_guardado))),
        Err(err) => Err((StatusCode::UNPROCESSABLE_ENTITY, err)),
    }
}