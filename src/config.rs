use std::env;

/// Configuración de la aplicación, leída una sola vez al arrancar desde el entorno (.env).
#[expect(dead_code)]
pub struct Config {
    /// URL base de CouchDB (ej. http://localhost:15984).
    pub couchdb_url: String,
    /// Usuario administrador de CouchDB.
    pub couchdb_user: String,
    /// Contraseña del administrador de CouchDB.
    pub couchdb_password: String,
    /// Puerto TCP en el que escucha la API.
    pub port: u16,
    /// Modo de autenticación (ej. "desactivado" en desarrollo).
    pub auth_mode: String,
    /// Secreto para firmar los JWT.
    pub jwt_secret: String,
}

impl Config {
    /// Construye la configuración desde variables de entorno.
    /// Devuelve error con el nombre de la primera variable que falte o sea inválida.
    pub fn desde_env() -> Result<Self, String> {
        Ok(Self {
            couchdb_url: requerida("COUCHDB_URL")?,
            couchdb_user: requerida("COUCHDB_USER")?,
            couchdb_password: requerida("COUCHDB_PASSWORD")?,
            // PORT llega como texto; se convierte a u16 (0-65535).
            port: requerida("PORT")?
                .parse()
                .map_err(|_| "PORT debe ser un número".to_string())?,
            auth_mode: requerida("AUTH_MODE")?,
            jwt_secret: requerida("JWT_SECRET")?,
        })
    }
}

/// Lee una variable de entorno obligatoria o devuelve un mensaje de error claro.
fn requerida(nombre: &str) -> Result<String, String> {
    env::var(nombre).map_err(|_| format!("Falta {nombre} en .env"))
}
