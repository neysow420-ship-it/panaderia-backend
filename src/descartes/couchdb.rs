use crate::descartes::dominio::Descarte;
use reqwest::Client;

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
