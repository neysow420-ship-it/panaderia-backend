use crate::descartes::couchdb;
use crate::descartes::dominio::Descarte;
use reqwest::Client;

/// Caso de uso: Registrar un nuevo descarte en el sistema
pub async fn registrar_descarte(
    client: &Client,
    couchdb_url: &str,
    mut descarte: Descarte,
) -> Result<Descarte, String> {
    // 1. Validar las reglas del dominio
    descarte.validar()?;

    // 2. Generar el _id si no lo trae (usando el formato del contrato)
    if descarte._id.is_empty() {
        let id = format!("descarte:{}", ulid::Ulid::generate());
    }

    // 3. Guardar el documento en la base de datos CouchDB (en la BD "panaderia")
    couchdb::guardar_descarte(client, couchdb_url, &descarte).await?;

    Ok(descarte)
}
