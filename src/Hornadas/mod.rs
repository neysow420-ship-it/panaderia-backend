//! # Módulo de Hornada
//!
//! Todo lo de hornadas vive en esta carpeta, para no modificar archivos de las
//! partes de los compañeros.
//!
//! | Archivo         | Capa            | Qué contiene |
//! |-----------------|-----------------|--------------|
//! | `dominio.rs`    | Dominio         | Datos de la hornada y reglas puras: validaciones, huella, vencimiento |
//! | `aplicacion.rs` | Aplicación      | Caso de uso `registrar_hornada`: idempotencia y producto existente |
//! | `couchdb.rs`    | Infraestructura | Lectura y escritura en CouchDB con Reqwest |
//! | `api.rs`        | API             | Ruta Axum `POST /hornadas` y códigos HTTP |
//! | `mod.rs`        | (este archivo)  | Une todo y entrega las rutas a `main.rs` |
//!
//! Flujo de una hornada:
//! `cliente → api.rs → aplicacion.rs → (dominio.rs + couchdb.rs) → CouchDB`
//!
//! ## Relaciones con otras partes
//!
//! **Código que hornada importa** (sin modificarlo):
//! - `crate::config::Config` → URL, usuario y contraseña de CouchDB (del `.env`).
//! - `crate::api::autenticacion::{Rol, Usuario}` → quién registra la hornada (parte de **Login**).
//!
//! **Datos que hornada lee de otros** (por el `_id` del documento estándar):
//! - **Producto y Config** → `catalogo/producto:*`: comprueba que el producto existe y
//!   toma `vida_util_dias` para el vencimiento por defecto.
//!   Cada hornada guarda `producto_id`, que apunta a ese producto.
//!
//! **Datos de hornada que leen otros**:
//! - **Vencimientos** → documentos `tipo: "hornada"` de `panaderia`: `_id` (como `lote_id`),
//!   `producto_id`, `unidades` y `fecha_vencimiento`.
//! - **Descarte** → `lote_id` apunta al `_id` de la hornada.
//! - **Cierre** → `producidas` = suma de `unidades` de las hornadas con `fecha` = día cerrado.
//!
//! En `main.rs` solo hacen falta `mod hornadas;` y `.merge(hornadas::rutas(&config))`
//! dentro de las rutas protegidas.

// Submódulos privados: solo se usan dentro de `hornadas`.
mod api;
mod aplicacion;
mod couchdb;
mod dominio;

use std::sync::Arc;

use axum::{Router, routing::post};

// Importado de la configuración general (no se modifica, solo se usa).
use crate::config::Config;
use couchdb::CouchDb;

/// Estado que comparten todas las peticiones de hornadas.
///
/// Es propio de hornadas: no se mezcla con el `Estado` general de `main.rs`,
/// así no hay que tocar ese struct. Va dentro de un `Arc` para compartirlo
/// entre peticiones sin copiarlo.
struct EstadoHornadas {
    /// Conexión a CouchDB.
    couch: CouchDb,
}

/// Prepara las rutas de hornadas. Se llama una vez al arrancar desde `main.rs`.
///
/// A diferencia de Ventas, **no carga nada al arrancar**: cada hornada lee el
/// producto directamente de CouchDB, así los cambios del catálogo se ven al
/// instante y el arranque no depende de que exista la base `catalogo`. Por eso
/// no es `async`.
///
/// `<S>` permite unir este router con el de `main.rs` sin importar qué estado use
/// `main.rs`: hornadas ya trae el suyo (`with_state`).
pub fn rutas<S: Clone + Send + Sync + 'static>(config: &Config) -> Router<S> {
    let couch = CouchDb::new(
        reqwest::Client::new(),
        &config.couchdb_url,
        &config.couchdb_user,
        &config.couchdb_password,
    )
    .expect("Configuración de CouchDB inválida");

    Router::new()
        .route("/hornadas", post(api::crear_hornada))
        .with_state(Arc::new(EstadoHornadas { couch }))
}
