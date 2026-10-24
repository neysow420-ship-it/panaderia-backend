//! # Acceso a CouchDB para ventas
//!
//! CouchDB se maneja por HTTP: cada documento es una URL
//! (`http://servidor/{base}/{_id}`). Aquí se usa **Reqwest** para hacer esas
//! peticiones:
//!
//! | Qué hace                    | Petición HTTP                                 |
//! |-----------------------------|-----------------------------------------------|
//! | Leer un documento           | `GET  /{base}/{_id}` → 200 o 404              |
//! | Crear un documento          | `PUT  /{base}/{_id}` → 201, o 409 si ya existe |
//! | Leer todo el catálogo       | `GET  /catalogo/_all_docs?include_docs=true`  |
//!
//! Aquí también se implementa [`RepositorioVentas`]: lo que el caso de uso
//! le pide a la base.
//!
//! Relaciones con otras partes (solo lectura, por el `_id` del documento estándar):
//! - **Producto y Config**: lee los documentos `producto:*` de la base `catalogo`.
//! - **Cierre**: revisa si existe `cierre:{fecha}` en la base `panaderia`.

use std::collections::HashMap;

use chrono::NaiveDate;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::aplicacion::{ErrorRepositorio, RepositorioVentas};
use super::dominio::ProductoCatalogo;
use super::dominio::Venta;

/// Base de documentos oficiales: ventas, hornadas, descartes, cierres, config.
pub const BD_PANADERIA: &str = "panaderia";
/// Base del catálogo de productos (la que la PWA replica para trabajar sin internet).
pub const BD_CATALOGO: &str = "catalogo";

/// Conexión a CouchDB.
///
/// `Client` de Reqwest guarda las conexiones abiertas y las reutiliza (pool),
/// así que se crea una vez al arrancar y se comparte entre peticiones.
#[derive(Clone)]
pub struct CouchDb {
    /// Cliente HTTP reutilizable.
    cliente: Client,
    /// URL base de CouchDB, ej. `http://localhost:15984` (de `COUCHDB_URL`).
    base: Url,
    /// Usuario de CouchDB (de `COUCHDB_USER`).
    usuario: String,
    /// Contraseña de CouchDB (de `COUCHDB_PASSWORD`). Nunca se imprime en logs.
    contrasena: String,
}

impl CouchDb {
    /// Crea la conexión. Falla si `COUCHDB_URL` no es una URL válida.
    /// (Todavía no se conecta: la primera petición real es al cargar el catálogo.)
    pub fn new(
        cliente: Client,
        url: &str,
        usuario: &str,
        contrasena: &str,
    ) -> Result<Self, String> {
        let base = Url::parse(url).map_err(|e| format!("COUCHDB_URL inválida: {e}"))?;
        Ok(Self {
            cliente,
            base,
            usuario: usuario.to_string(),
            contrasena: contrasena.to_string(),
        })
    }

    /// Arma la URL `{base}/{segmento1}/{segmento2}...`.
    ///
    /// Cada segmento se codifica por separado, así un `_id` con caracteres
    /// especiales no rompe la URL. `pop_if_empty` evita una `//` si la base
    /// termina en `/`.
    fn url(&self, segmentos: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("URL base válida")
            .pop_if_empty()
            .extend(segmentos);
        url
    }

    /// Lee un documento por su `_id` y lo convierte al tipo `T`.
    ///
    /// - 200 → `Ok(Some(documento))`
    /// - 404 → `Ok(None)` (no existe; no es un error)
    /// - Otro código o sin conexión → `Err(NoDisponible)`
    ///
    /// Es genérica (`T`) para leer tanto ventas como cierres u otros documentos.
    pub async fn obtener<T: DeserializeOwned>(
        &self,
        bd: &str,
        id: &str,
    ) -> Result<Option<T>, ErrorRepositorio> {
        let resp = self
            .cliente
            .get(self.url(&[bd, id]))
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .map_err(no_disponible)?;

        match resp.status() {
            StatusCode::OK => resp.json().await.map(Some).map_err(no_disponible),
            StatusCode::NOT_FOUND => Ok(None),
            otro => Err(ErrorRepositorio::NoDisponible(format!(
                "CouchDB respondió {otro} al leer {bd}/{id}"
            ))),
        }
    }

    /// Crea un documento nuevo con `PUT /{bd}/{id}`.
    ///
    /// - 201 (creado) o 202 (aceptado) → `Ok(())`. Solo aquí se considera guardado.
    /// - 409 → `Err(Conflicto)`: ya existe un documento con ese `_id`. Así CouchDB
    ///   impide que la misma venta se guarde dos veces.
    /// - Otro código o sin conexión → `Err(NoDisponible)`.
    pub async fn crear<T: Serialize>(
        &self,
        bd: &str,
        id: &str,
        doc: &T,
    ) -> Result<(), ErrorRepositorio> {
        let resp = self
            .cliente
            .put(self.url(&[bd, id]))
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .json(doc)
            .send()
            .await
            .map_err(no_disponible)?;

        match resp.status() {
            StatusCode::CREATED | StatusCode::ACCEPTED => Ok(()),
            StatusCode::CONFLICT => Err(ErrorRepositorio::Conflicto),
            otro => Err(ErrorRepositorio::NoDisponible(format!(
                "CouchDB respondió {otro} al crear {bd}/{id}"
            ))),
        }
    }

    /// Relación con **Producto y Config**: lee todos los productos de `catalogo`.
    ///
    /// Se llama al arrancar y antes de cada venta, así un producto o un precio
    /// creado desde fuera de esta API se aplica sin reiniciar el servidor.
    ///
    /// Devuelve un `HashMap` de `producto_id` → producto, para buscar rápido.
    /// Se saltan los `_design/...` y cualquier documento que no sea `tipo: "producto"`
    /// o que no tenga los campos que venta necesita.
    pub async fn cargar_catalogo(
        &self,
    ) -> Result<HashMap<String, ProductoCatalogo>, ErrorRepositorio> {
        // Forma de la respuesta de `_all_docs`: { "rows": [ { "doc": {...} }, ... ] }
        #[derive(Deserialize)]
        struct Fila {
            doc: Option<serde_json::Value>,
        }
        #[derive(Deserialize)]
        struct Respuesta {
            rows: Vec<Fila>,
        }

        // `include_docs=true` hace que cada fila traiga el documento completo.
        let mut url = self.url(&[BD_CATALOGO, "_all_docs"]);
        url.query_pairs_mut().append_pair("include_docs", "true");

        let resp: Respuesta = self
            .cliente
            .get(url)
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .and_then(|r| r.error_for_status()) // 4xx/5xx → error
            .map_err(no_disponible)?
            .json()
            .await
            .map_err(no_disponible)?;

        Ok(resp
            .rows
            .into_iter()
            .filter_map(|f| f.doc) // solo filas con documento
            .filter(|d| d["tipo"] == "producto") // solo productos
            .filter_map(|d| serde_json::from_value::<ProductoCatalogo>(d).ok())
            .map(|p| (p.id.clone(), p))
            .collect())
    }
}

/// Convierte un error de Reqwest (sin conexión, timeout, JSON roto) en `NoDisponible`.
fn no_disponible(e: reqwest::Error) -> ErrorRepositorio {
    ErrorRepositorio::NoDisponible(e.to_string())
}

/// Lo que el caso de uso le pide a la base, hecho con CouchDB.
impl RepositorioVentas for CouchDb {
    /// Lee `panaderia/venta:{ULID}`.
    async fn obtener_venta(&self, id: &str) -> Result<Option<Venta>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, id).await
    }

    /// Crea `panaderia/venta:{ULID}`.
    async fn crear_venta(&self, venta: &Venta) -> Result<(), ErrorRepositorio> {
        self.crear(BD_PANADERIA, &venta.id, venta).await
    }

    /// Relación con **Cierre**: según el documento estándar, el cierre de un día
    /// tiene `_id` fijo `cierre:{AAAA-MM-DD}`. Basta un GET para saber si existe;
    /// no se lee su contenido, así venta no depende de los campos del cierre.
    async fn existe_cierre(&self, fecha: NaiveDate) -> Result<bool, ErrorRepositorio> {
        let id = format!("cierre:{fecha}");
        Ok(self
            .obtener::<serde_json::Value>(BD_PANADERIA, &id)
            .await?
            .is_some())
    }
}
