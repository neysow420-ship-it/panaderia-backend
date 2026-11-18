mod gmail;

use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
};
use chrono::NaiveDate;
use reqwest::{RequestBuilder, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    Estado,
    api::autenticacion::{Rol, Usuario},
    config::Config,
};
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub(crate) const MAX_ADJUNTO: usize = 16 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Factura {
    numero: String,
    proveedor: String,
    fecha: NaiveDate,
    lineas: Vec<Linea>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Linea {
    sku: String,
    descripcion: String,
    cantidad: u32,
    #[serde(rename = "costo_unitario", with = "costo_decimal")]
    costo_centavos: u64,
}

impl Factura {
    fn valida(&self) -> bool {
        texto_valido(&self.numero, 100)
            && texto_valido(&self.proveedor, 200)
            && !self.lineas.is_empty()
            && self.lineas.iter().all(|linea| {
                texto_valido(&linea.sku, 100)
                    && texto_valido(&linea.descripcion, 300)
                    && linea.cantidad > 0
                    && (1..=1_000_000_000_000).contains(&linea.costo_centavos)
            })
            && self.total_centavos().is_some()
    }

    fn total_centavos(&self) -> Option<u64> {
        self.lineas
            .iter()
            .try_fold(0_u64, |total, linea| {
                total.checked_add(
                    linea
                        .costo_centavos
                        .checked_mul(u64::from(linea.cantidad))?,
                )
            })
            .filter(|total| *total <= 9_007_199_254_740_991)
    }

    fn id(&self) -> String {
        let identidad = serde_json::to_vec(&(&self.proveedor, &self.numero))
            .expect("Identidad de factura serializable");
        format!("factura:{:x}", Sha256::digest(identidad))
    }
}

mod costo_decimal {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
    use serde_json::{Number, value::RawValue};

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let numero = Box::<RawValue>::deserialize(deserializer)?;
        let texto = numero.get();
        let (unidades, fraccion) = texto.split_once('.').unwrap_or((texto, ""));
        let invalido = || {
            de::Error::custom("costo_unitario debe ser un número decimal con hasta dos decimales")
        };
        if fraccion.len() > 2 || !texto.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
            return Err(invalido());
        }
        let unidades: u64 = unidades.parse().map_err(|_| invalido())?;
        let centavos = match fraccion.len() {
            0 => 0,
            1 => fraccion.parse::<u64>().map_err(|_| invalido())? * 10,
            _ => fraccion.parse().map_err(|_| invalido())?,
        };
        unidades
            .checked_mul(100)
            .and_then(|valor| valor.checked_add(centavos))
            .ok_or_else(invalido)
    }

    pub(super) fn serialize<S: Serializer>(
        centavos: &u64,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let texto = format!("{}.{:02}", centavos / 100, centavos % 100);
        let numero: Number = texto.parse().map_err(serde::ser::Error::custom)?;
        numero.serialize(serializer)
    }
}

fn texto_valido(texto: &str, maximo: usize) -> bool {
    !texto.is_empty()
        && texto.len() <= maximo
        && texto.trim() == texto
        && !texto.chars().any(char::is_control)
}

#[derive(Deserialize)]
pub(crate) struct Pagina {
    #[serde(default)]
    offset: u32,
}

pub(crate) async fn listar(
    State(estado): State<Arc<Estado>>,
    Extension(usuario): Extension<Usuario>,
    Query(pagina): Query<Pagina>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !matches!(usuario.rol, Rol::Encargado) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"codigo": "prohibido"}))));
    }

    let filas = listar_en_couch(&estado, pagina.offset)
        .await
        .map_err(|error| {
            eprintln!("GET /facturas: {error}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"codigo": "no_disponible"})),
            )
        })?;
    let siguiente = (filas.len() > 50).then(|| u64::from(pagina.offset) + 50);
    Ok(Json(json!({
        "facturas": filas.into_iter().take(50).collect::<Vec<_>>(),
        "siguiente_offset": siguiente,
    })))
}

async fn listar_en_couch(estado: &Estado, offset: u32) -> Result<Vec<Value>, reqwest::Error> {
    let mut url = couch_url(&estado.config, "_all_docs");
    url.query_pairs_mut()
        .append_pair("startkey", "\"factura:\"")
        .append_pair("endkey", "\"factura;\"")
        .append_pair("include_docs", "true")
        .append_pair("limit", "51")
        .append_pair("skip", &offset.to_string());

    // Los documentos almacenados incluyen metadatos de CouchDB.
    #[derive(Deserialize)]
    struct Documento {
        factura: Value,
    }
    #[derive(Deserialize)]
    struct Documentos {
        rows: Vec<FilaDocumento>,
    }
    #[derive(Deserialize)]
    struct FilaDocumento {
        doc: Documento,
    }

    let documentos: Documentos = couch_auth(estado.cliente.get(url), &estado.config)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(documentos
        .rows
        .into_iter()
        .map(|fila| fila.doc.factura)
        .collect())
}

fn couch_url(config: &Config, recurso: &str) -> Url {
    let mut url = Url::parse(&config.couchdb_url).expect("URL de CouchDB válida");
    url.path_segments_mut()
        .expect("URL de CouchDB jerárquica")
        .pop_if_empty()
        .extend(["panaderia", recurso]);
    url
}

fn couch_auth(peticion: RequestBuilder, config: &Config) -> RequestBuilder {
    peticion.basic_auth(&config.couchdb_user, Some(&config.couchdb_password))
}

#[derive(PartialEq)]
enum GuardarFactura {
    Creada,
    Duplicada,
    Conflicto,
}

fn parsear_factura(datos: &[u8]) -> Option<Factura> {
    if datos.len() > MAX_ADJUNTO {
        return None;
    }
    serde_json::from_slice::<Factura>(datos)
        .ok()
        .filter(Factura::valida)
}

pub(crate) async fn importar(
    State(estado): State<Arc<Estado>>,
    cabeceras: HeaderMap,
    datos: Bytes,
) -> (StatusCode, Json<Value>) {
    let respuesta = |estado, codigo| (estado, Json(json!({"codigo": codigo})));
    let clave = std::env::var("FACTURAS_IMPORTACION_CLAVE").unwrap_or_default();
    if clave.is_empty() {
        return respuesta(StatusCode::NOT_FOUND, "no_disponible");
    }

    let recibida = cabeceras
        .get(header::AUTHORIZATION)
        .and_then(|valor| valor.to_str().ok())
        .and_then(|valor| valor.strip_prefix("Bearer "))
        .unwrap_or("");
    let coincide = Sha256::digest(recibida.as_bytes()).ct_eq(&Sha256::digest(clave.as_bytes()));
    if !bool::from(coincide) {
        return respuesta(StatusCode::UNAUTHORIZED, "no_autorizado");
    }

    let Some(factura) = parsear_factura(&datos) else {
        return respuesta(StatusCode::UNPROCESSABLE_ENTITY, "factura_invalida");
    };
    match guardar(&estado.cliente, &estado, &factura).await {
        Ok(GuardarFactura::Creada) => respuesta(StatusCode::CREATED, "factura_creada"),
        Ok(GuardarFactura::Duplicada) => respuesta(StatusCode::OK, "factura_duplicada"),
        Ok(GuardarFactura::Conflicto) => respuesta(StatusCode::CONFLICT, "factura_en_conflicto"),
        Err(error) => {
            eprintln!("Importación de factura: {error}");
            respuesta(StatusCode::SERVICE_UNAVAILABLE, "no_disponible")
        }
    }
}

async fn guardar(
    cliente: &reqwest::Client,
    estado: &Estado,
    factura: &Factura,
) -> Result<GuardarFactura, reqwest::Error> {
    let url = couch_url(&estado.config, &factura.id());
    let respuesta = couch_auth(cliente.put(url.clone()), &estado.config)
        .json(&documento(factura))
        .send()
        .await?;
    if respuesta.status() == StatusCode::CONFLICT {
        #[derive(Deserialize)]
        struct Guardada {
            factura: Value,
        }
        let guardada: Guardada = couch_auth(cliente.get(url), &estado.config)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut datos = guardada.factura;
        if let Some(objeto) = datos.as_object_mut() {
            objeto.remove("total_centavos");
        }
        if serde_json::from_value::<Factura>(datos).ok().as_ref() == Some(factura) {
            return Ok(GuardarFactura::Duplicada);
        }
        return Ok(GuardarFactura::Conflicto);
    }
    respuesta.error_for_status()?;
    Ok(GuardarFactura::Creada)
}

fn documento(factura: &Factura) -> Value {
    let mut datos = json!(factura);
    datos["total_centavos"] = json!(factura.total_centavos().expect("Total de factura validado"));
    json!({"tipo": "factura", "factura": datos})
}

pub(crate) fn iniciar_worker(estado: Arc<Estado>) {
    match gmail::Configuracion::desde_env() {
        Ok(Some(config)) => {
            tokio::spawn(gmail::ejecutar(estado, config));
        }
        Ok(None) => {}
        Err(error) => eprintln!("Worker de facturas deshabilitado: {error}"),
    }
}
