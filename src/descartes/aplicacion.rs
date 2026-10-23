use crate::descartes::couchdb;
use crate::descartes::dominio::Descarte;
use reqwest::Client;

/// Error del caso de uso de descartes.
#[derive(Debug)]
pub enum ErrorDescarte {
    /// Datos inválidos → **422**.
    Validacion(String),
    /// CouchDB no responde o rechazó la escritura → **503**.
    NoDisponible(String),
}

/// Caso de uso: registrar un nuevo descarte en el sistema.
pub async fn registrar_descarte(
    client: &Client,
    couchdb_url: &str,
    usuario_couchdb: &str,
    contrasena_couchdb: &str,
    mut descarte: Descarte,
) -> Result<Descarte, ErrorDescarte> {
    // 1. Validar las reglas del dominio.
    descarte.validar().map_err(ErrorDescarte::Validacion)?;

    // 2. Generar el _id con el formato del contrato: `descarte:{ULID}`.
    if descarte._id.is_empty() {
        let id = format!("descarte:{}", ulid::Ulid::generate());
    }

    // 3. Guardar el documento en la base `panaderia`.
    couchdb::guardar_descarte(
        client,
        couchdb_url,
        usuario_couchdb,
        contrasena_couchdb,
        &descarte,
    )
    .await
    .map_err(ErrorDescarte::NoDisponible)?;

    Ok(descarte)
}
