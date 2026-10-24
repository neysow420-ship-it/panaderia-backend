//! # Módulo de Venta
//!
//! Todo lo de ventas vive en esta carpeta, para no modificar archivos de las
//! partes de los compañeros.
//!
//! | Archivo         | Capa            | Qué contiene |
//! |-----------------|-----------------|--------------|
//! | `dominio.rs`    | Dominio         | Datos de la venta y reglas puras: validaciones, huella, importes |
//! | `aplicacion.rs` | Aplicación      | Caso de uso `registrar_venta`: idempotencia y día cerrado |
//! | `couchdb.rs`    | Infraestructura | Lectura y escritura en CouchDB con Reqwest |
//! | `api.rs`        | API             | Ruta Axum `POST /ventas` y códigos HTTP |
//! | `mod.rs`        | (este archivo)  | Une todo y entrega las rutas a `main.rs` |
//!
//! Flujo de una venta:
//! `cliente → api.rs → aplicacion.rs → (dominio.rs + couchdb.rs) → CouchDB`
//!
//! ## Relaciones con otras partes
//!
//! **Código que venta importa** (sin modificarlo):
//! - `crate::config::Config` → URL, usuario y contraseña de CouchDB (del `.env`).
//! - `crate::api::autenticacion::Usuario` → quién hace la venta (parte de **Login**).
//!
//! **Datos que venta lee de otros** (por el `_id` del documento estándar):
//! - **Producto y Config** → `catalogo/producto:*`: `precio_centavos` y `activo`.
//!   Cada línea de la venta guarda `producto_id`, que apunta a ese producto.
//! - **Cierre** → `panaderia/cierre:{fecha}`: si existe, ese día ya está cerrado.
//!
//! **Datos de venta que leen otros**:
//! - **Cierre** y **Vencimientos** → `vendidas` por día y producto con la vista
//!   `_design/ventas/_view/ventas_por_dia_producto` (`infra/couchdb/ventas/`).
//!
//! **Sin relación directa**: Horneada y Descarte (se cuadran en el cierre).
//!
//! En `main.rs` solo hacen falta `mod ventas;` y `.merge(ventas::rutas(&config).await)`.

// Submódulos privados: solo se usan dentro de `ventas`.
mod api;
mod aplicacion;
mod couchdb;
mod dominio;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{Router, routing::post};

// Importado de la configuración general (no se modifica, solo se usa).
use crate::config::Config;
use couchdb::CouchDb;
use dominio::ProductoCatalogo;

/// Estado que comparten todas las peticiones de ventas.
///
/// Es propio de ventas: no se mezcla con el `Estado` general de `main.rs`,
/// así no hay que tocar ese struct. Va dentro de un `Arc` para compartirlo
/// entre peticiones sin copiarlo.
struct EstadoVentas {
    /// Conexión a CouchDB.
    couch: CouchDb,
    /// Último catálogo bueno: `producto_id` → producto.
    ///
    /// Se refresca antes de cada venta porque los productos y los precios
    /// pueden cambiar desde fuera de esta API (Fauxton, otro servicio, etc.).
    /// El `Mutex` evita que dos ventas recarguen el catálogo a la vez.
    catalogo: tokio::sync::Mutex<HashMap<String, ProductoCatalogo>>,
}

impl EstadoVentas {
    /// Devuelve el catálogo más reciente.
    ///
    /// Si CouchDB no responde, se usa la última copia buena para no rechazar
    /// una venta por un fallo momentáneo del catálogo.
    async fn catalogo_actual(&self) -> HashMap<String, ProductoCatalogo> {
        let mut catalogo = self.catalogo.lock().await;

        match self.couch.cargar_catalogo().await {
            Ok(productos) => {
                *catalogo = productos;
            }
            Err(error) => {
                eprintln!(
                    "Ventas: no se pudo refrescar el catálogo, se usa el último cargado: {error:?}"
                );
            }
        }

        catalogo.clone()
    }
}

/// Prepara las rutas de ventas. Se llama una vez al arrancar desde `main.rs`.
///
/// 1. Crea la conexión a CouchDB con los datos del `.env`.
/// 2. Carga el catálogo de productos en memoria (relación con **Producto y Config**).
/// 3. Devuelve un `Router` con `POST /ventas` que `main.rs` une a sus rutas
///    protegidas, para que pase por el middleware de autenticación.
///
/// El catálogo se vuelve a leer antes de cada venta (ver
/// [`EstadoVentas::catalogo_actual`]), para que los productos y precios
/// recién creados o modificados se apliquen sin reiniciar la API.
///
/// `<S>` permite unir este router con el de `main.rs` sin importar qué estado use
/// `main.rs`: ventas ya trae el suyo (`with_state`).
///
/// Si CouchDB no responde o no existe la base `catalogo`, el programa se detiene
/// con un mensaje claro, porque sin precios no se puede vender.
pub async fn rutas<S: Clone + Send + Sync + 'static>(config: &Config) -> Router<S> {
    let couch = CouchDb::new(
        reqwest::Client::new(),
        &config.couchdb_url,
        &config.couchdb_user,
        &config.couchdb_password,
    )
    .expect("Configuración de CouchDB inválida");

    let catalogo = couch
        .cargar_catalogo()
        .await
        .expect("No se pudo cargar el catálogo desde CouchDB (¿existe la base 'catalogo'?)");
    println!(
        "Ventas: catálogo inicial cargado con {} productos",
        catalogo.len()
    );

    Router::new()
        .route("/ventas", post(api::crear_venta))
        .with_state(Arc::new(EstadoVentas {
            couch,
            catalogo: tokio::sync::Mutex::new(catalogo),
        }))
}
