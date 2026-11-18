use std::{env, sync::Arc, time::Duration};

use base64::{
    Engine,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use reqwest::{Client, Url};
use serde::Deserialize;

use super::{GuardarFactura, MAX_ADJUNTO, guardar, parsear_factura};
use crate::Estado;

type Error = Box<dyn std::error::Error + Send + Sync>;

pub(super) struct Configuracion {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    buzon: String,
    consulta: String,
}

impl Configuracion {
    pub(super) fn desde_env() -> Result<Option<Self>, Error> {
        if env::var("FACTURAS_GMAIL_ACTIVO").as_deref() != Ok("true") {
            return Ok(None);
        }
        fn requerida(nombre: &str) -> Result<String, Error> {
            env::var(nombre)
                .ok()
                .filter(|valor| !valor.trim().is_empty())
                .ok_or_else(|| format!("Falta {nombre}").into())
        }
        let buzon = requerida("FACTURAS_GMAIL_BUZON")?;
        let remitente = requerida("FACTURAS_GMAIL_REMITENTE")?;
        if [&buzon, &remitente].into_iter().any(|correo| {
            correo.matches('@').count() != 1
                || correo.starts_with('@')
                || correo.ends_with('@')
                || !correo
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
        }) {
            return Err("El buzón y el remitente deben ser direcciones individuales".into());
        }
        Ok(Some(Self {
            client_id: requerida("FACTURAS_GMAIL_CLIENT_ID")?,
            client_secret: requerida("FACTURAS_GMAIL_CLIENT_SECRET")?,
            refresh_token: requerida("FACTURAS_GMAIL_REFRESH_TOKEN")?,
            buzon,
            consulta: format!("label:facturas-panaderia from:{remitente} filename:factura.json"),
        }))
    }

    fn url(&self, segmentos: &[&str]) -> Url {
        let mut url = Url::parse("https://gmail.googleapis.com/gmail/v1/users/").unwrap();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .extend([self.buzon.as_str(), "messages"])
            .extend(segmentos.iter().copied());
        url
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagina {
    #[serde(default)]
    messages: Vec<MensajeId>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct MensajeId {
    id: String,
}

#[derive(Deserialize)]
struct Mensaje {
    payload: Parte,
}

#[derive(Deserialize)]
struct Parte {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    parts: Vec<Parte>,
    body: Cuerpo,
}

impl Parte {
    fn adjuntos<'a>(&'a self, encontrados: &mut Vec<&'a Cuerpo>) {
        if self.filename == "factura.json" {
            encontrados.push(&self.body);
        }
        for parte in &self.parts {
            parte.adjuntos(encontrados);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cuerpo {
    size: usize,
    data: Option<String>,
    attachment_id: Option<String>,
}

pub(super) async fn ejecutar(estado: Arc<Estado>, config: Configuracion) {
    let cliente = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("Cliente HTTP del worker de facturas");
    let mut intervalo = tokio::time::interval(Duration::from_secs(60));
    intervalo.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Evita volver a descargar adjuntos resueltos durante esta ejecución.
    let mut recientes = std::collections::HashSet::new();
    loop {
        intervalo.tick().await;
        if recientes.len() >= 10_000 {
            recientes.clear();
        }
        if let Err(error) = revisar(&cliente, &estado, &config, &mut recientes).await {
            eprintln!("Worker de facturas: {error}");
        }
    }
}

async fn revisar(
    cliente: &Client,
    estado: &Estado,
    config: &Configuracion,
    recientes: &mut std::collections::HashSet<String>,
) -> Result<(), Error> {
    let token: Token = cliente
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("refresh_token", config.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut pagina: Option<String> = None;
    loop {
        let mut url = config.url(&[]);
        url.query_pairs_mut()
            .append_pair("q", &config.consulta)
            .append_pair("maxResults", "100");
        if let Some(cursor) = &pagina {
            url.query_pairs_mut().append_pair("pageToken", cursor);
        }
        let resultado: Pagina = cliente
            .get(url)
            .bearer_auth(&token.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        for mensaje in resultado.messages {
            if !recientes.contains(&mensaje.id) {
                match procesar(cliente, estado, config, &token.access_token, &mensaje.id).await {
                    Ok(()) => {
                        recientes.insert(mensaje.id);
                    }
                    Err(error) => eprintln!("Worker de facturas, mensaje {}: {error}", mensaje.id),
                }
            }
        }
        pagina = resultado.next_page_token;
        if pagina.is_none() {
            return Ok(());
        }
    }
}

async fn procesar(
    cliente: &Client,
    estado: &Estado,
    config: &Configuracion,
    token: &str,
    id: &str,
) -> Result<(), Error> {
    let mut url = config.url(&[id]);
    url.query_pairs_mut().append_pair("format", "full");
    let mensaje: Mensaje = cliente
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut adjuntos = Vec::new();
    mensaje.payload.adjuntos(&mut adjuntos);
    let [adjunto] = adjuntos.as_slice() else {
        eprintln!("Factura rechazada ({id}): se requiere un solo adjunto factura.json");
        return Ok(());
    };
    if adjunto.size > MAX_ADJUNTO {
        eprintln!("Factura rechazada ({id}): adjunto mayor a 16 KiB");
        return Ok(());
    }

    let descargado;
    let cuerpo = if let Some(attachment_id) = &adjunto.attachment_id {
        descargado = cliente
            .get(config.url(&[id, "attachments", attachment_id]))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json::<Cuerpo>()
            .await?;
        &descargado
    } else {
        adjunto
    };
    let factura = cuerpo
        .data
        .as_deref()
        .filter(|datos| datos.len() <= MAX_ADJUNTO.div_ceil(3) * 4)
        .and_then(|datos| {
            URL_SAFE_NO_PAD
                .decode(datos)
                .or_else(|_| URL_SAFE.decode(datos))
                .ok()
        })
        .and_then(|datos| parsear_factura(&datos));

    let Some(factura) = factura else {
        eprintln!("Factura rechazada ({id}): JSON inválido o fuera del contrato");
        return Ok(());
    };
    if guardar(cliente, estado, &factura).await? == GuardarFactura::Conflicto {
        eprintln!("Factura rechazada ({id}): el proveedor y número ya existen con otros datos");
    }
    Ok(())
}

#[cfg(test)]
mod pruebas {
    use crate::facturas::{Factura, documento};

    fn datos(costo: &str) -> String {
        format!(
            r#"{{"numero":"FAC-0001","proveedor":"Molinos del Sur","fecha":"2026-11-18",
            "lineas":[{{"sku":"harina-50kg","descripcion":"Harina de trigo","cantidad":4,"costo_unitario":{costo}}},
                      {{"sku":"azucar-25kg","descripcion":"Azúcar","cantidad":2,"costo_unitario":21.0}}]}}"#
        )
    }

    #[test]
    fn conserva_las_lineas_y_calcula_el_total_en_centavos() {
        let factura: Factura = serde_json::from_str(&datos("28.5")).unwrap();
        assert!(factura.valida());
        let mut guardada = documento(&factura)["factura"].clone();
        assert_eq!(guardada["total_centavos"], 15_600);
        guardada.as_object_mut().unwrap().remove("total_centavos");
        assert_eq!(
            serde_json::from_value::<Factura>(guardada).unwrap(),
            factura
        );
        assert_eq!(
            serde_json::from_str::<Factura>(&datos("28.50")).unwrap(),
            factura
        );
    }

    #[test]
    fn rechaza_costos_con_precision_o_tipo_invalido() {
        for costo in [
            "28.501",
            "28.5000000000000001",
            "2.85e1",
            "\"28.50\"",
            "-1",
            r#"{"$serde_json::private::Number":"28.50"}"#,
        ] {
            assert!(
                serde_json::from_str::<Factura>(&datos(costo)).is_err(),
                "{costo}"
            );
        }
    }
}
