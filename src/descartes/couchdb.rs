use crate::descartes::dominio::Descarte;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

/// Guarda el documento de descarte en la base `panaderia` de CouchDB.
pub async fn guardar_descarte(
    client: &Client,
    couchdb_url: &str,
    usuario: &str,
    contrasena: &str,
    descarte: &Descarte,
) -> Result<(), String> {
    let url = format!("{}/panaderia/{}", couchdb_url, descarte._id);

    let response = client
        .put(&url)
        .basic_auth(usuario, Some(contrasena))
        .json(descarte)
        .send()
        .await
        .map_err(|e| format!("Error de red al conectar con CouchDB: {}", e))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!(
            "Error al guardar descarte en CouchDB [{status}]: {body}"
        ))
    }
}

/// Lee los descartes de un día concreto.
///
/// Usa `_all_docs` con el rango de claves `"descarte:"` …
/// `"descarte:\u{fff0}"`, igual que el módulo de cierres, para no depender de
/// que la vista esté instalada.
pub async fn listar_descartes(
    client: &Client,
    couchdb_url: &str,
    usuario: &str,
    contrasena: &str,
    fecha: &str,
) -> Result<Vec<Descarte>, String> {
    #[derive(Deserialize)]
    struct Fila {
        doc: Option<Value>,
    }

    #[derive(Deserialize)]
    struct Respuesta {
        rows: Vec<Fila>,
    }

    let mut url = reqwest::Url::parse(&format!("{}/panaderia/_all_docs", couchdb_url))
        .map_err(|e| format!("COUCHDB_URL inválida al leer descartes: {}", e))?;

    url.query_pairs_mut()
        .append_pair("include_docs", "true")
        .append_pair("startkey", "\"descarte:\"")
        .append_pair("endkey", "\"descarte:\u{fff0}\"");

    let respuesta: Respuesta = client
        .get(url)
        .basic_auth(usuario, Some(contrasena))
        .send()
        .await
        .map_err(|e| format!("Error de red al leer descartes: {}", e))?
        .error_for_status()
        .map_err(|e| format!("CouchDB rechazó la lectura de descartes: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Respuesta inválida de CouchDB al leer descartes: {}", e))?;

    let mut descartes = Vec::new();

    for fila in respuesta.rows {
        let Some(documento) = fila.doc else {
            continue;
        };

        if documento.get("tipo").and_then(Value::as_str) != Some("descarte") {
            continue;
        }

        let convertido: Descarte = serde_json::from_value(documento)
            .map_err(|e| format!("Documento de descarte mal formado: {}", e))?;

        if convertido.fecha == fecha {
            descartes.push(convertido);
        }
    }

    descartes.sort_by(|a, b| a.creado_en.cmp(&b.creado_en));

    Ok(descartes)
}
