use crate::descartes::dominio::Descarte;
use reqwest::Client;

/// Guarda el documento de descarte en la base de datos de CouchDB
pub async fn guardar_descarte(
    client: &Client,
    couchdb_url: &str,
    descarte: &Descarte,
) -> Result<(), String> {
    // Construimos la URL apuntando a la base de datos "panaderia"
    let url = format!("{}/panaderia/{}", couchdb_url, descarte._id);

    let response = client
        .put(&url)
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
