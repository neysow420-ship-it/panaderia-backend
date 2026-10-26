use chrono::NaiveDate;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

use super::aplicacion::{
    CierreGuardado, ConfigMerma, DescarteMerma, ErrorRepositorio, HornadaMerma, LineaVentaReporte,
    ProductoMerma, RepositorioReportes, VentaReporte,
};

const BD_PANADERIA: &str = "panaderia";
const BD_CATALOGO: &str = "catalogo";

/// Representación mínima de una venta oficial.
#[derive(Deserialize)]
struct DocumentoVenta {
    fecha: NaiveDate,
    lineas: Vec<LineaDocumentoVenta>,
}

#[derive(Deserialize)]
struct LineaDocumentoVenta {
    producto_id: String,
    cantidad: i64,
    subtotal_centavos: i64,
}

/// Conexión de lectura a CouchDB para ambos reportes.
pub struct CouchDb {
    cliente: Client,
    base: Url,
    usuario: String,
    contrasena: String,
}

impl CouchDb {
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

    fn url(&self, segmentos: &[&str]) -> Url {
        let mut url = self.base.clone();

        url.path_segments_mut()
            .expect("URL base válida")
            .pop_if_empty()
            .extend(segmentos);

        url
    }

    /// Lee un documento específico. Un 404 significa que no existe.
    async fn obtener<T: DeserializeOwned>(
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

    /// Lista documentos por prefijo de _id, verificando también su tipo.
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

            let documento = serde_json::from_value(documento).map_err(|error| {
                ErrorRepositorio::NoDisponible(format!(
                    "Documento de tipo {tipo} mal formado: {error}"
                ))
            })?;

            documentos.push(documento);
        }

        Ok(documentos)
    }
}

fn no_disponible(error: reqwest::Error) -> ErrorRepositorio {
    ErrorRepositorio::NoDisponible(error.to_string())
}

impl RepositorioReportes for CouchDb {
    // ── Reporte de ventas ────────────────────────────────────────────────

    async fn listar_ventas(
        &self,
        desde: NaiveDate,
        hasta: NaiveDate,
    ) -> Result<Vec<VentaReporte>, ErrorRepositorio> {
        let documentos: Vec<DocumentoVenta> = self
            .listar_por_prefijo(BD_PANADERIA, "venta:", "venta")
            .await?;

        Ok(documentos
            .into_iter()
            .filter(|venta| venta.fecha >= desde && venta.fecha <= hasta)
            .map(|venta| {
                let lineas = venta
                    .lineas
                    .into_iter()
                    .map(|linea| LineaVentaReporte {
                        producto_id: linea.producto_id,
                        cantidad: linea.cantidad,
                        subtotal_centavos: linea.subtotal_centavos,
                    })
                    .collect();

                VentaReporte {
                    fecha: venta.fecha,
                    lineas,
                }
            })
            .collect())
    }

    // ── Reporte de merma ─────────────────────────────────────────────────

    async fn obtener_cierre(
        &self,
        fecha: NaiveDate,
    ) -> Result<Option<CierreGuardado>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, &format!("cierre:{fecha}")).await
    }

    async fn listar_hornadas(
        &self,
        desde: NaiveDate,
        hasta: NaiveDate,
    ) -> Result<Vec<HornadaMerma>, ErrorRepositorio> {
        let documentos: Vec<HornadaMerma> = self
            .listar_por_prefijo(BD_PANADERIA, "hornada:", "hornada")
            .await?;

        Ok(documentos
            .into_iter()
            .filter(|hornada| hornada.fecha >= desde && hornada.fecha <= hasta)
            .collect())
    }

    async fn listar_descartes(
        &self,
        desde: NaiveDate,
        hasta: NaiveDate,
    ) -> Result<Vec<DescarteMerma>, ErrorRepositorio> {
        let documentos: Vec<DescarteMerma> = self
            .listar_por_prefijo(BD_PANADERIA, "descarte:", "descarte")
            .await?;

        let mut filtrados = Vec::new();

        for descarte in documentos {
            let fecha =
                NaiveDate::parse_from_str(&descarte.fecha, "%Y-%m-%d").map_err(|error| {
                    ErrorRepositorio::NoDisponible(format!("Descarte con fecha inválida: {error}"))
                })?;

            if fecha >= desde && fecha <= hasta {
                filtrados.push(descarte);
            }
        }

        Ok(filtrados)
    }

    async fn listar_productos(&self) -> Result<Vec<ProductoMerma>, ErrorRepositorio> {
        self.listar_por_prefijo(BD_CATALOGO, "producto:", "producto")
            .await
    }

    async fn obtener_config(&self) -> Result<Option<ConfigMerma>, ErrorRepositorio> {
        self.obtener(BD_PANADERIA, "config:vigente").await
    }
}
