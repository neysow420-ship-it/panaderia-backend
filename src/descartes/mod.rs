pub mod api;
pub mod aplicacion;
pub mod couchdb;
pub mod dominio;

use axum::{Router, routing::post};

use crate::config::Config;

/// Prepara las rutas de descartes con su propio estado (URL y credenciales de
/// CouchDB). Se llama una vez al arrancar desde `main.rs` y se une a las rutas
/// protegidas para que pase por el middleware de autenticación.
pub fn rutas<S: Clone + Send + Sync + 'static>(config: &Config) -> Router<S> {
    let estado = api::AppState {
        couchdb_url: config.couchdb_url.clone(),
        couchdb_user: config.couchdb_user.clone(),
        couchdb_password: config.couchdb_password.clone(),
        http_client: reqwest::Client::new(),
    };

    Router::new()
        .route("/descartes", post(api::post_descarte))
        .with_state(estado)
}
