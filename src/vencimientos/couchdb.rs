///Capa de infraestructura. Realiza las peticiones HTTP con reqwest a la base de datos panaderia en CouchDB para extraer los documentos de tipo hornada

use reqwest::Client;
use serde_json::Value;

/// Repositorio de infraestructura para comunicarse directamente con CouchDB vía HTTP.
pub struct VencimientosCouchRepository {
    client: Client,
    base_url: String,
    db_name: String,
}

impl VencimientosCouchRepository {
    /// Inicializa una nueva instancia del repositorio CouchDB.
    pub fn new(client: Client, base_url: String, db_name: String) -> Self {
        Self { client, base_url, db_name }
    }

    /// Realiza una consulta Mango Query (_find) a CouchDB para traer todas las hornadas registradas.
    pub async fn obtener_hornadas_activas(&self) -> Result<Vec<Value>, String> {
        let url = format!("{}/{}/_find", self.base_url, self.db_name);
        
        // Criterio de búsqueda en JSON para filtrar documentos de tipo "hornada"
        let query = serde_json::json!({
            "selector": {
                "tipo": "hornada"
            }
        });

        // Petición POST a la API REST de CouchDB
        let response = self.client
            .post(&url)
            .json(&query)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let body = response
            .json::<Value>()
            .await
            .map_err(|e| e.to_string())?;

        // Extraemos el arreglo de documentos "docs" del JSON recibido
        let docs = body["docs"].as_array().cloned().unwrap_or_default();
        Ok(docs)
    }
}