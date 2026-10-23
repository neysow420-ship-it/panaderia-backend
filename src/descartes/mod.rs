// Mantenemos disponibles las capas del módulo de descartes.
pub mod api;
pub mod aplicacion;
pub mod couchdb;
pub mod dominio;

// Herramientas para construir las rutas HTTP.
use axum::{Router, routing::post};

// Configuración general del backend.
use crate::config::Config;

/// Construye las rutas HTTP del módulo de descartes.
///
/// El parámetro genérico S permite integrar este router
/// con el router principal sin compartir directamente su estado.
pub fn rutas<S>(config: &Config) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // Creamos el estado que necesita el endpoint de descartes.
    let estado = api::AppState {
        couchdb_url: config.couchdb_url.clone(),
        http_client: reqwest::Client::new(),
    };

    // Registramos POST /descartes e inyectamos el estado propio.
    Router::new()
        .route("/descartes", post(api::post_descarte))
        .with_state(estado)
}
