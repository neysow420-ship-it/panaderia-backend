//! Capa de infraestructura: consulta las hornadas en CouchDB.

use reqwest::Client;
use serde_json::Value;

/// Repositorio de infraestructura para comunicarse con CouchDB vía HTTP.
pub struct VencimientosCouchRepository {
    client: Client,
    base_url: String,
    usuario: String,
    contrasena: String,
    db_name: String,
}

impl VencimientosCouchRepository {
    /// Inicializa el repositorio con la URL y las credenciales de CouchDB.
    pub fn new(
        client: Client,
        base_url: String,
        usuario: String,
        contrasena: String,
        db_name: String,
    ) -> Self {
        Self {
            client,
            base_url,
            usuario,
            contrasena,
            db_name,
        }
    }

    /// Consulta Mango (`_find`) para traer todas las hornadas registradas.
    pub async fn obtener_hornadas_activas(&self) -> Result<Vec<Value>, String> {
        let url = format!("{}/{}/_find", self.base_url, self.db_name);

        let query = serde_json::json!({
            "selector": { "tipo": "hornada" }
        });

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .json(&query)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;

        let body = response.json::<Value>().await.map_err(|e| e.to_string())?;

        Ok(body["docs"].as_array().cloned().unwrap_or_default())
    }
}
