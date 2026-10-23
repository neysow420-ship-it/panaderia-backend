//! # Módulo de Producto y Config
//!
//! Todo lo del catálogo y la configuración vive en esta carpeta, para no
//! modificar archivos de las partes de los compañeros.
//!
//! | Archivo         | Capa            | Qué contiene |
//! |-----------------|-----------------|--------------|
//! | `dominio.rs`    | Dominio         | Datos de `producto` y `config:vigente`, y reglas puras: validaciones |
//! | `aplicacion.rs` | Aplicación      | Casos de uso: listar, crear, actualizar producto; leer y guardar config |
//! | `couchdb.rs`    | Infraestructura | Lectura y escritura en CouchDB con Reqwest |
//! | `api.rs`        | API             | Rutas Axum `/productos` y `/config`, y códigos HTTP |
//! | `mod.rs`        | (este archivo)  | Une todo y entrega las rutas a `main.rs` |
//!
//! Flujo de una petición:
//! `cliente → api.rs → aplicacion.rs → (dominio.rs + couchdb.rs) → CouchDB`
//!
//! ## Relaciones con otras partes
//!
//! **Código que este módulo importa** (sin modificarlo):
//! - `crate::config::Config` → URL, usuario y contraseña de CouchDB (del `.env`).
//!
//! **Quién lee lo que este módulo guarda**:
//! - **Ventas** → `catalogo/producto:*` (`precio_centavos`, `activo`), cargado en memoria al arrancar.
//! - **Horneada, Descarte, Vencimientos** → `producto_id` y `vida_util_dias`.
//! - **Cierre** → `costos` y `modo_produccion` de `panaderia/config:vigente`.
//!
//! En `main.rs` solo hacen falta `mod productos;` y `.merge(productos::rutas(&config))`
//! dentro de las rutas protegidas.

// Submódulos privados: solo se usan dentro de `productos`.
mod api;
mod aplicacion;
mod couchdb;
mod dominio;

use std::sync::Arc;

use axum::{Router, routing::get};

// Importado de la configuración general (no se modifica, solo se usa).
use crate::config::Config;
use couchdb::CouchDb;

/// Estado que comparten todas las peticiones de este módulo.
///
/// Es propio de productos: no se mezcla con el `Estado` general de `main.rs`.
/// Va dentro de un `Arc` para compartirlo entre peticiones sin copiarlo.
struct EstadoProductos {
    /// Conexión a CouchDB.
    couch: CouchDb,
}

/// Prepara las rutas de productos y config. Se llama una vez al arrancar desde `main.rs`.
///
/// A diferencia de Ventas, **no carga nada al arrancar**: cada petición lee
/// CouchDB directamente, así los cambios se ven al instante. Por eso no es `async`.
///
/// `<S>` permite unir este router con el de `main.rs` sin importar qué estado use
/// `main.rs`: este módulo ya trae el suyo (`with_state`).
pub fn rutas<S: Clone + Send + Sync + 'static>(config: &Config) -> Router<S> {
    let couch = CouchDb::new(
        reqwest::Client::new(),
        &config.couchdb_url,
        &config.couchdb_user,
        &config.couchdb_password,
    )
    .expect("Configuración de CouchDB inválida");

    Router::new()
        .route("/productos", get(api::listar).post(api::crear))
        // En Axum 0.7 el parámetro se escribe `/:slug` en vez de `/{slug}`.
        .route("/productos/{slug}", get(api::obtener).put(api::actualizar))
        .route("/config", get(api::leer_config).put(api::guardar_config))
        .with_state(Arc::new(EstadoProductos { couch }))
}
