use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde::Serialize;

use crate::Estado;
use crate::config::ModoAuth;

/// Rol del usuario que hace la petición.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Rol {
    Encargado,
}

/// Usuario autenticado; el middleware lo deja en la petición
/// para que los endpoints lo lean sin saber cómo se autenticó.
#[derive(Clone, Debug)]
pub struct Usuario {
    pub rol: Rol,
}

/// Middleware de autenticación según AUTH_MODE.
pub async fn autenticar(
    State(estado): State<Arc<Estado>>,
    mut peticion: Request,
    siguiente: Next,
) -> Result<Response, StatusCode> {
    match estado.config.auth_mode {
        // Implementación falsa: todos son "encargado", sin pedir login.
        ModoAuth::Desactivado => {
            peticion.extensions_mut().insert(Usuario {
                rol: Rol::Encargado,
            });
            Ok(siguiente.run(peticion).await)
        }
        // El login real aún no existe: se rechaza en vez de dejar pasar.
        ModoAuth::Activo => Err(StatusCode::NOT_IMPLEMENTED),
    }
}
