//! Módulo de vencimientos: calcula el reporte a partir de las hornadas
//! guardadas en CouchDB.

pub mod api;
pub mod aplicacion;
pub mod couchdb;
pub mod dominio;

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::config::Config;
use aplicacion::VencimientoCasoUso;
use couchdb::VencimientosCouchRepository;

/// Estado que comparten las peticiones del reporte.
pub struct EstadoVencimientos {
    caso: VencimientoCasoUso,
}

/// Prepara `GET /vencimientos` con su propia conexión autenticada a CouchDB.
pub fn rutas<S: Clone + Send + Sync + 'static>(config: &Config) -> Router<S> {
    let repo = VencimientosCouchRepository::new(
        reqwest::Client::new(),
        config.couchdb_url.clone(),
        config.couchdb_user.clone(),
        config.couchdb_password.clone(),
        "panaderia".to_string(),
    );

    Router::new()
        .route("/vencimientos", get(api::obtener_vencimientos_handler))
        .with_state(Arc::new(EstadoVencimientos {
            caso: VencimientoCasoUso::new(repo),
        }))
}
