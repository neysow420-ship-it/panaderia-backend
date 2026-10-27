use std::env;

/// Modo de autenticación de la API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModoAuth {
    Activo,
    Desactivado,
}

impl ModoAuth {
    /// Convierte el texto de AUTH_MODE; rechaza cualquier otro valor.
    fn desde_texto(texto: &str) -> Result<Self, String> {
        match texto {
            "activo" => Ok(Self::Activo),
            "desactivado" => Ok(Self::Desactivado),
            otro => Err(format!(
                "AUTH_MODE debe ser 'activo' o 'desactivado', no '{otro}'"
            )),
        }
    }
}

/// Configuración de la aplicación, leída una sola vez al arrancar desde el entorno (.env).
pub struct Config {
    /// URL base de CouchDB (ej. http://localhost:15984).
    pub couchdb_url: String,
    /// Usuario de aplicación para acceder a CouchDB.
    pub couchdb_user: String,
    /// Contraseña del usuario de aplicación.
    pub couchdb_password: String,
    /// Puerto TCP en el que escucha la API.
    pub port: u16,
    /// Modo de autenticación (`activo` o `desactivado`).
    pub auth_mode: ModoAuth,
    /// Secreto para firmar los JWT.
    pub jwt_secret: String,
    /// Orígenes del navegador autorizados por CORS.
    pub cors_origins: Vec<String>,
}

/// Orígenes que usa el frontend en desarrollo cuando no se define `CORS_ORIGINS`.
const ORIGENES_POR_DEFECTO: [&str; 4] = [
    "http://localhost:5173",
    "http://127.0.0.1:5173",
    "http://localhost:4173",
    "http://127.0.0.1:4173",
];

impl Config {
    /// Construye la configuración desde variables de entorno.
    /// Devuelve error con el nombre de la primera variable que falte o sea inválida.
    pub fn desde_env() -> Result<Self, String> {
        Ok(Self {
            couchdb_url: requerida("COUCHDB_URL")?,
            couchdb_user: requerida("BACKEND_USER")?,
            couchdb_password: requerida("BACKEND_PASSWORD")?,
            // PORT llega como texto; se convierte a u16 (0-65535).
            port: requerida("PORT")?
                .parse()
                .map_err(|_| "PORT debe ser un número".to_string())?,
            auth_mode: ModoAuth::desde_texto(&requerida("AUTH_MODE")?)?,
            jwt_secret: requerida("JWT_SECRET")?,
            cors_origins: origenes(env::var("CORS_ORIGINS").ok())?,
        })
    }
}

/// Lee `CORS_ORIGINS` (lista separada por comas) o usa los orígenes locales.
///
/// Se rechazan valores vacíos y comodines: con `Authorization` el navegador
/// exige un origen concreto, y `*` daría una falsa sensación de seguridad.
fn origenes(valor: Option<String>) -> Result<Vec<String>, String> {
    let Some(texto) = valor else {
        return Ok(ORIGENES_POR_DEFECTO
            .iter()
            .map(|origen| origen.to_string())
            .collect());
    };

    let lista: Vec<String> = texto
        .split(',')
        .map(str::trim)
        .filter(|origen| !origen.is_empty())
        .map(str::to_string)
        .collect();

    if lista.is_empty() {
        return Err("CORS_ORIGINS no puede estar vacío".to_string());
    }

    for origen in &lista {
        if origen == "*" {
            return Err("CORS_ORIGINS no admite '*': indica los orígenes exactos".to_string());
        }
        if !(origen.starts_with("http://") || origen.starts_with("https://")) {
            return Err(format!(
                "CORS_ORIGINS contiene un origen inválido: '{origen}'"
            ));
        }
    }

    Ok(lista)
}

/// Lee una variable de entorno obligatoria o devuelve un mensaje de error claro.
fn requerida(nombre: &str) -> Result<String, String> {
    env::var(nombre).map_err(|_| format!("Falta {nombre} en .env"))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn sin_cors_usa_los_origenes_locales() {
        assert_eq!(origenes(None).unwrap(), ORIGENES_POR_DEFECTO);
    }

    #[test]
    fn cors_acepta_lista_con_espacios() {
        let lista = origenes(Some(
            "http://localhost:5173, https://panaderia.duckdns.org ".to_string(),
        ))
        .unwrap();

        assert_eq!(
            lista,
            vec![
                "http://localhost:5173".to_string(),
                "https://panaderia.duckdns.org".to_string()
            ]
        );
    }

    #[test]
    fn cors_rechaza_comodin_y_origenes_invalidos() {
        assert!(origenes(Some("*".to_string())).is_err());
        assert!(origenes(Some("panaderia.duckdns.org".to_string())).is_err());
        assert!(origenes(Some(" , ".to_string())).is_err());
    }
}
