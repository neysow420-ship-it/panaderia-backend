//! # Acceso a CouchDB para productos y config
//!
//! CouchDB se maneja por HTTP: cada documento es una URL
//! (`http://servidor/{base}/{_id}`). Aquí se usa **Reqwest** para hacer esas
//! peticiones:
//!
//! | Qué hace                      | Petición HTTP                                           |
//! |-------------------------------|---------------------------------------------------------|
//! | Leer un documento             | `GET  /{base}/{_id}` → 200 o 404                        |
//! | Crear o reemplazar            | `PUT  /{base}/{_id}` → 201/202, o 409 si choca         |
//! | Listar productos              | `GET  /catalogo/_all_docs?include_docs=true&startkey=…` |
//!
//! Sobre el 409 del `PUT`: al **crear** significa "ese `_id` ya existe"; al
//! **reemplazar** significa "la `_rev` que mandaste ya no es la actual". Para el
//! código es lo mismo (`Conflicto`); el caso de uso decide qué significa.
//!
//! Aquí también se implementa [`RepositorioProductos`].
//!
//! - Productos viven en la base `catalogo` (la que la PWA replica para trabajar sin internet).
//! - La configuración vive en la base `panaderia`, `_id` fijo `config:vigente`.

use reqwest::{Client, StatusCode, Url};
use serde::{Serialize, de::DeserializeOwned};

use super::aplicacion::{ErrorRepositorio, RepositorioProductos};
use super::dominio::{ConfigVigente, ID_CONFIG, PREFIJO_PRODUCTO, Producto};

/// Base de documentos oficiales: ventas, hornadas, descartes, cierres, config.
pub const BD_PANADERIA: &str = "panaderia";
/// Base del catálogo de productos.
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
    /// (Todavía no se conecta: la primera petición real es la de la primera llamada a la API.)
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

    /// Guarda un documento con `PUT /{bd}/{id}`: sirve para **crear** (sin `_rev`)
    /// y para **reemplazar** (con la `_rev` actual dentro del documento).
    ///
    /// - 201 (creado) o 202 (aceptado) → `Ok(())`. Solo aquí se considera guardado.
    /// - 409 → `Err(Conflicto)`: el `_id` ya existe (al crear) o la `_rev` quedó vieja.
    /// - Otro código o sin conexión → `Err(NoDisponible)`.
    pub async fn guardar<T: Serialize>(
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
                "CouchDB respondió {otro} al guardar {bd}/{id}"
            ))),
        }
    }
}

/// Convierte un error de Reqwest (sin conexión, timeout, JSON roto) en `NoDisponible`.
fn no_disponible(e: reqwest::Error) -> ErrorRepositorio {
    ErrorRepositorio::NoDisponible(e.to_string())
}

/// Lo que los casos de uso le piden a la base, hecho con CouchDB.
impl RepositorioProductos for CouchDb {
    /// Lee `catalogo/producto:{slug}`.
    async fn obtener_producto(&self, id: &str) -> Result<Option<Producto>, ErrorRepositorio> {
        self.obtener(BD_CATALOGO, id).await
    }

    /// Lista todos los productos de `catalogo`.
    ///
    /// `_all_docs` devuelve los documentos ordenados por `_id`. Con `startkey` y
    /// `endkey` se piden solo los que empiezan con `producto:` (el `\u{fff0}` es un
    /// carácter muy alto que marca el final del rango), así se saltan los `_design/...`.
    /// Por si acaso, se filtra también por `tipo: "producto"` y se descartan los
    /// documentos que no tengan los campos requeridos.
    async fn listar_productos(&self) -> Result<Vec<Producto>, ErrorRepositorio> {
        // Forma de la respuesta de `_all_docs`: { "rows": [ { "doc": {...} }, ... ] }
        #[derive(serde::Deserialize)]
        struct Fila {
            doc: Option<serde_json::Value>,
        }
        #[derive(serde::Deserialize)]
        struct Respuesta {
            rows: Vec<Fila>,
        }

        let mut url = self.url(&[BD_CATALOGO, "_all_docs"]);
        url.query_pairs_mut()
            .append_pair("include_docs", "true")
            // CouchDB espera las llaves como JSON, por eso van entre comillas.
            .append_pair("startkey", &format!("\"{PREFIJO_PRODUCTO}\""))
            .append_pair("endkey", &format!("\"{PREFIJO_PRODUCTO}\u{fff0}\""));

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
            .filter_map(|d| serde_json::from_value::<Producto>(d).ok())
            .collect())
    }

    /// Crea `catalogo/producto:{slug}`. 409 = el slug ya existe.
    async fn crear_producto(&self, producto: &Producto) -> Result<(), ErrorRepositorio> {
        self.guardar(BD_CATALOGO, &producto.id, producto).await
    }

    /// Reemplaza `catalogo/producto:{slug}`; el documento trae su `_rev`.
    async fn actualizar_producto(&self, producto: &Producto) -> Result<(), ErrorRepositorio> {
        self.guardar(BD_CATALOGO, &producto.id, producto).await
    }

    /// Lee `panaderia/config:vigente`.
    async fn obtener_config(&self) -> Result<Option<ConfigVigente>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, ID_CONFIG).await
    }

    /// Crea o reemplaza `panaderia/config:vigente` (con `_rev` si ya existía).
    async fn guardar_config(&self, config: &ConfigVigente) -> Result<(), ErrorRepositorio> {
        self.guardar(BD_PANADERIA, ID_CONFIG, config).await
    }
}
