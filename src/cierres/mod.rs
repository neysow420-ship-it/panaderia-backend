mod api;
mod aplicacion;
mod couchdb;
mod dominio;

use std::sync::Arc;

use axum::{Router, routing::post};

use crate::config::Config;
use couchdb::CouchDb;

/// Estado propio del módulo de cierres.
struct EstadoCierres {
    couch: CouchDb,
}

/// Prepara la ruta POST /cierre.
pub fn rutas<S: Clone + Send + Sync + 'static>(config: &Config) -> Router<S> {
    let couch = CouchDb::new(
        reqwest::Client::new(),
        &config.couchdb_url,
        &config.couchdb_user,
        &config.couchdb_password,
    )
    .expect("Configuración de CouchDB inválida");

    Router::new()
        .route("/cierre", post(api::crear_cierre))
        .with_state(Arc::new(EstadoCierres { couch }))
}
