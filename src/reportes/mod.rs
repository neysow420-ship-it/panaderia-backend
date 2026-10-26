mod api;
mod aplicacion;
mod couchdb;
mod dominio;

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::config::Config;
use couchdb::CouchDb;

struct EstadoReportes {
    couch: CouchDb,
}

/// Prepara las rutas del módulo de reportes.
pub fn rutas<S>(config: &Config) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let couch = CouchDb::new(
        reqwest::Client::new(),
        &config.couchdb_url,
        &config.couchdb_user,
        &config.couchdb_password,
    )
    .expect("Configuración de CouchDB inválida");

    Router::new()
        .route("/reportes/ventas", get(api::obtener_reporte_ventas))
        .route("/merma", get(api::obtener_reporte_merma))
        .with_state(Arc::new(EstadoReportes { couch }))
}
