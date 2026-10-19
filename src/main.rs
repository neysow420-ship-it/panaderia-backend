mod api;
mod config;
use std::net::SocketAddr;
use std::sync::Arc;

use api::autenticacion::{Usuario, autenticar};
use axum::{Extension, Json, Router, extract::State, http::StatusCode, middleware, routing::get};
use config::Config;
use serde_json::{Value, json};

/// Estado compartido entre todas las peticiones.
/// Se envuelve en `Arc` para compartirlo entre tareas sin copiarlo.
struct Estado {
    /// Cliente HTTP reutilizable (mantiene el pool de conexiones a CouchDB).
    cliente: reqwest::Client,
    /// Configuración cargada al arrancar.
    config: Config,
}

/// GET /api/salud
/// Comprueba que la API puede hablar con CouchDB con las credenciales configuradas.
/// Responde 200 con la versión de CouchDB, o 502 si CouchDB no responde o rechaza el acceso.
async fn salud(State(estado): State<Arc<Estado>>) -> Result<Json<Value>, StatusCode> {
    // Petición autenticada a la raíz de CouchDB.
    let respuesta = estado
        .cliente
        .get(&estado.config.couchdb_url)
        .basic_auth(
            &estado.config.couchdb_user,
            Some(&estado.config.couchdb_password),
        )
        .send()
        .await
        // Convierte respuestas 4xx/5xx de CouchDB en error.
        .and_then(|r| r.error_for_status())
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    // CouchDB responde con JSON de bienvenida: {"couchdb":"Welcome","version":"3.3.3",...}
    let info: Value = respuesta
        .json()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(json!({ "estado": "ok", "couchdb": info["version"] })))
}

/// GET /api/yo
/// Devuelve el rol del usuario que dejó el middleware de autenticación.
async fn yo(Extension(usuario): Extension<Usuario>) -> Json<Value> {
    Json(json!({ "rol": usuario.rol }))
}

#[tokio::main]
async fn main() {
    // Carga el .env en las variables de entorno; si no existe (ej. en Docker) se ignora.
    dotenvy::dotenv().ok();

    // Falla al arrancar si falta alguna variable, antes de aceptar peticiones.
    let config = Config::desde_env().expect("Configuración inválida");
    let puerto = config.port;

    let estado = Arc::new(Estado {
        cliente: reqwest::Client::new(),
        config,
    });

    // /api/salud es pública; las rutas protegidas pasan por el middleware.
    let protegidas = Router::new()
        .route("/api/yo", get(yo))
        .route_layer(middleware::from_fn_with_state(estado.clone(), autenticar));

    let app: Router = Router::new()
        .route("/api/salud", get(salud))
        .merge(protegidas)
        .with_state(estado);

    // 0.0.0.0 permite conexiones desde fuera del contenedor.
    let direccion = SocketAddr::from(([0, 0, 0, 0], puerto));
    let listener: tokio::net::TcpListener = tokio::net::TcpListener::bind(direccion)
        .await
        .expect("No se pudo abrir el puerto");

    axum::serve(listener, app)
        .await
        .expect("Error del servidor");
}
