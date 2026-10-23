//! # Acceso a CouchDB para hornadas
//!
//! CouchDB se maneja por HTTP: cada documento es una URL
//! (`http://servidor/{base}/{_id}`). Aquí se usa **Reqwest** para hacer esas
//! peticiones:
//!
//! | Qué hace                    | Petición HTTP                                 |
//! |-----------------------------|-----------------------------------------------|
//! | Leer un documento           | `GET  /{base}/{_id}` → 200 o 404              |
//! | Crear un documento          | `PUT  /{base}/{_id}` → 201, o 409 si ya existe |
//!
//! Aquí también se implementa [`RepositorioHornadas`]: lo que el caso de uso
//! le pide a la base.
//!
//! Relaciones con otras partes (solo lectura, por el `_id` del documento estándar):
//! - **Producto y Config**: lee `catalogo/producto:{slug}` para comprobar que el
//!   producto existe y obtener su `vida_util_dias`.

use reqwest::{Client, StatusCode, Url};
use serde::{Serialize, de::DeserializeOwned};

use super::aplicacion::{ErrorRepositorio, RepositorioHornadas};
use super::dominio::{Hornada, ProductoCatalogo};

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
    /// (Todavía no se conecta: la primera petición real es la de la primera hornada.)
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
    /// Es genérica (`T`) para leer tanto hornadas como productos.
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
    ///   impide que la misma hornada se guarde dos veces.
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
}

/// Convierte un error de Reqwest (sin conexión, timeout, JSON roto) en `NoDisponible`.
fn no_disponible(e: reqwest::Error) -> ErrorRepositorio {
    ErrorRepositorio::NoDisponible(e.to_string())
}

/// Lo que el caso de uso le pide a la base, hecho con CouchDB.
impl RepositorioHornadas for CouchDb {
    /// Lee `panaderia/hornada:{ULID}`.
    async fn obtener_hornada(&self, id: &str) -> Result<Option<Hornada>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, id).await
    }

    /// Crea `panaderia/hornada:{ULID}`.
    async fn crear_hornada(&self, hornada: &Hornada) -> Result<(), ErrorRepositorio> {
        self.crear(BD_PANADERIA, &hornada.id, hornada).await
    }

    /// Relación con **Producto y Config**: lee `catalogo/producto:{slug}`.
    ///
    /// Se lee en cada petición (sin caché) para que un producto nuevo del catálogo
    /// se pueda hornear de inmediato. El `producto_id` ya viene validado como
    /// `producto:{slug}`, así que aquí no se puede leer otro tipo de documento.
    async fn obtener_producto(
        &self,
        id: &str,
    ) -> Result<Option<ProductoCatalogo>, ErrorRepositorio> {
        self.obtener(BD_CATALOGO, id).await
    }
}
