use chrono::NaiveDate;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::aplicacion::{
    ConfigCierre, ErrorRepositorio, HornadaDia, ProductoCierre, RepositorioCierres, VentaDia,
};
use super::dominio::Cierre;

const BD_PANADERIA: &str = "panaderia";
const BD_CATALOGO: &str = "catalogo";

/// Conexión de CouchDB dedicada al módulo de cierres.
#[derive(Clone)]
pub struct CouchDb {
    cliente: Client,
    base: Url,
    usuario: String,
    contrasena: String,
}

impl CouchDb {
    /// Crea la conexión con la configuración existente del backend.
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

    /// Construye una URL codificando cada segmento de la ruta.
    fn url(&self, segmentos: &[&str]) -> Url {
        let mut url = self.base.clone();

        url.path_segments_mut()
            .expect("URL base válida")
            .pop_if_empty()
            .extend(segmentos);

        url
    }

    /// Lee un documento por su identificador.
    ///
    /// 404 significa que no existe; los demás errores se informan
    /// como una dependencia no disponible.
    pub async fn obtener<T: DeserializeOwned>(
        &self,
        bd: &str,
        id: &str,
    ) -> Result<Option<T>, ErrorRepositorio> {
        let respuesta = self
            .cliente
            .get(self.url(&[bd, id]))
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .map_err(no_disponible)?;

        match respuesta.status() {
            StatusCode::OK => respuesta.json().await.map(Some).map_err(no_disponible),
            StatusCode::NOT_FOUND => Ok(None),
            otro => Err(ErrorRepositorio::NoDisponible(format!(
                "CouchDB respondió {otro} al leer {bd}/{id}"
            ))),
        }
    }

    /// Crea un documento nuevo, sin reemplazar uno existente.
    ///
    /// CouchDB rechaza con 409 los documentos cuyo _id ya existe.
    pub async fn crear<T: Serialize>(
        &self,
        bd: &str,
        id: &str,
        documento: &T,
    ) -> Result<(), ErrorRepositorio> {
        let respuesta = self
            .cliente
            .put(self.url(&[bd, id]))
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .json(documento)
            .send()
            .await
            .map_err(no_disponible)?;

        match respuesta.status() {
            StatusCode::CREATED | StatusCode::ACCEPTED => Ok(()),
            StatusCode::CONFLICT => Err(ErrorRepositorio::Conflicto),
            otro => Err(ErrorRepositorio::NoDisponible(format!(
                "CouchDB respondió {otro} al crear {bd}/{id}"
            ))),
        }
    }

    /// Lee los documentos de un tipo mediante el rango de sus identificadores.
    ///
    /// Si un documento oficial del tipo esperado está mal formado, no lo
    /// ignoramos: hacerlo podría producir un cierre con cantidades incorrectas.
    async fn listar_por_prefijo<T: DeserializeOwned>(
        &self,
        bd: &str,
        prefijo: &str,
        tipo: &str,
    ) -> Result<Vec<T>, ErrorRepositorio> {
        #[derive(Deserialize)]
        struct Fila {
            doc: Option<Value>,
        }

        #[derive(Deserialize)]
        struct Respuesta {
            rows: Vec<Fila>,
        }

        let mut url = self.url(&[bd, "_all_docs"]);

        let inicio = format!("\"{prefijo}\"");
        let fin = format!("\"{prefijo}\u{fff0}\"");

        url.query_pairs_mut()
            .append_pair("include_docs", "true")
            .append_pair("startkey", &inicio)
            .append_pair("endkey", &fin);

        let respuesta: Respuesta = self
            .cliente
            .get(url)
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .map_err(no_disponible)?
            .error_for_status()
            .map_err(no_disponible)?
            .json()
            .await
            .map_err(no_disponible)?;

        let mut documentos = Vec::new();

        for fila in respuesta.rows {
            let Some(documento) = fila.doc else {
                continue;
            };

            if documento.get("tipo").and_then(Value::as_str) != Some(tipo) {
                continue;
            }

            let convertido = serde_json::from_value(documento).map_err(|error| {
                ErrorRepositorio::NoDisponible(format!(
                    "Documento de tipo {tipo} mal formado: {error}"
                ))
            })?;

            documentos.push(convertido);
        }

        Ok(documentos)
    }
}

fn no_disponible(error: reqwest::Error) -> ErrorRepositorio {
    ErrorRepositorio::NoDisponible(error.to_string())
}

impl RepositorioCierres for CouchDb {
    async fn obtener_cierre(&self, fecha: NaiveDate) -> Result<Option<Cierre>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, &format!("cierre:{fecha}")).await
    }

    async fn listar_hornadas(&self, fecha: NaiveDate) -> Result<Vec<HornadaDia>, ErrorRepositorio> {
        let documentos: Vec<HornadaDia> = self
            .listar_por_prefijo(BD_PANADERIA, "hornada:", "hornada")
            .await?;

        Ok(documentos
            .into_iter()
            .filter(|documento| documento.fecha == fecha)
            .collect())
    }

    async fn listar_ventas(&self, fecha: NaiveDate) -> Result<Vec<VentaDia>, ErrorRepositorio> {
        let documentos: Vec<VentaDia> = self
            .listar_por_prefijo(BD_PANADERIA, "venta:", "venta")
            .await?;

        Ok(documentos
            .into_iter()
            .filter(|documento| documento.fecha == fecha)
            .collect())
    }

    async fn listar_productos(&self) -> Result<Vec<ProductoCierre>, ErrorRepositorio> {
        self.listar_por_prefijo(BD_CATALOGO, "producto:", "producto")
            .await
    }

    async fn obtener_config(&self) -> Result<Option<ConfigCierre>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, "config:vigente").await
    }

    async fn crear_cierre(&self, cierre: &Cierre) -> Result<(), ErrorRepositorio> {
        self.crear(BD_PANADERIA, &cierre.id, cierre).await
    }
}
