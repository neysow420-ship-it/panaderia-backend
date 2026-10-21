//! # Rutas HTTP de Producto y Config
//!
//! Son las "puertas de entrada": reciben la petición, llaman al caso de uso
//! de [`aplicacion`] y convierten el resultado en una respuesta HTTP.
//!
//! | Ruta                    | Qué hace                              | Éxito |
//! |-------------------------|---------------------------------------|-------|
//! | `GET  /productos`       | Lista (`?solo_activos=true` opcional) | 200   |
//! | `POST /productos`       | Crea un producto                      | 201   |
//! | `GET  /productos/{slug}`| Lee un producto                       | 200   |
//! | `PUT  /productos/{slug}`| Reemplaza los datos de un producto    | 200   |
//! | `GET  /config`          | Lee la configuración vigente          | 200   |
//! | `PUT  /config`          | Crea o reemplaza la configuración     | 200   |
//!
//! | Error                                   | Código | Cuerpo |
//! |-----------------------------------------|--------|--------|
//! | JSON mal formado o datos inválidos      | 422    | `{ codigo, errores }` |
//! | No existe el producto / la config       | 404    | `{ codigo, errores: [] }` |
//! | Slug repetido o edición simultánea      | 409    | `{ codigo, errores }` |
//! | CouchDB no disponible                   | 503    | `{ codigo, errores: [] }` |
//!
//! Relación con **Login**: las rutas van dentro de las protegidas de `main.rs`,
//! así que antes de llegar aquí pasan por el middleware de autenticación.
//! (Todavía no se distingue el rol: cualquier usuario autenticado puede editar.)

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::EstadoProductos;
use super::aplicacion::{self, ErrorCatalogo};
use super::dominio::{DatosProducto, ErrorCampo, SolicitudConfig, SolicitudCrearProducto};

/// Parámetros opcionales de `GET /productos`.
#[derive(Deserialize)]
pub struct FiltroProductos {
    /// Si es `true`, solo devuelve productos activos (lo que usa la caja para vender).
    #[serde(default)]
    solo_activos: bool,
}

/// `GET /productos`.
pub async fn listar(
    State(estado): State<Arc<EstadoProductos>>,
    Query(filtro): Query<FiltroProductos>,
) -> Response {
    match aplicacion::listar_productos(&estado.couch, filtro.solo_activos).await {
        Ok(mut productos) => {
            // `_rev` es un dato interno de CouchDB que el cliente no necesita.
            productos.iter_mut().for_each(|p| p.rev = None);
            exito(StatusCode::OK, productos)
        }
        Err(e) => traducir(e),
    }
}

/// `GET /productos/{slug}`.
pub async fn obtener(
    State(estado): State<Arc<EstadoProductos>>,
    Path(slug): Path<String>,
) -> Response {
    match aplicacion::obtener_producto(&estado.couch, &slug).await {
        Ok(mut p) => {
            p.rev = None;
            exito(StatusCode::OK, p)
        }
        Err(e) => traducir(e),
    }
}

/// `POST /productos`.
///
/// El cuerpo se recibe como `Result` para responder con nuestro formato de error
/// si el JSON viene mal, en vez del mensaje por defecto de Axum.
pub async fn crear(
    State(estado): State<Arc<EstadoProductos>>,
    cuerpo: Result<Json<SolicitudCrearProducto>, JsonRejection>,
) -> Response {
    let Json(solicitud) = match cuerpo {
        Ok(c) => c,
        Err(rechazo) => return json_invalido(rechazo),
    };
    match aplicacion::crear_producto(&estado.couch, &solicitud).await {
        Ok(mut p) => {
            p.rev = None;
            exito(StatusCode::CREATED, p)
        }
        Err(e) => traducir(e),
    }
}

/// `PUT /productos/{slug}`.
pub async fn actualizar(
    State(estado): State<Arc<EstadoProductos>>,
    Path(slug): Path<String>,
    cuerpo: Result<Json<DatosProducto>, JsonRejection>,
) -> Response {
    let Json(datos) = match cuerpo {
        Ok(c) => c,
        Err(rechazo) => return json_invalido(rechazo),
    };
    match aplicacion::actualizar_producto(&estado.couch, &slug, &datos).await {
        Ok(mut p) => {
            p.rev = None;
            exito(StatusCode::OK, p)
        }
        Err(e) => traducir(e),
    }
}

/// `GET /config`.
pub async fn leer_config(State(estado): State<Arc<EstadoProductos>>) -> Response {
    match aplicacion::obtener_config(&estado.couch).await {
        Ok(mut c) => {
            c.rev = None;
            exito(StatusCode::OK, c)
        }
        Err(e) => traducir(e),
    }
}

/// `PUT /config`.
pub async fn guardar_config(
    State(estado): State<Arc<EstadoProductos>>,
    cuerpo: Result<Json<SolicitudConfig>, JsonRejection>,
) -> Response {
    let Json(solicitud) = match cuerpo {
        Ok(c) => c,
        Err(rechazo) => return json_invalido(rechazo),
    };
    match aplicacion::guardar_config(&estado.couch, &solicitud).await {
        Ok(mut c) => {
            c.rev = None;
            exito(StatusCode::OK, c)
        }
        Err(e) => traducir(e),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Respuestas
// ─────────────────────────────────────────────────────────────────────────────

/// Respuesta de éxito: el documento (o la lista) tal cual, como JSON.
fn exito<T: Serialize>(codigo: StatusCode, valor: T) -> Response {
    (codigo, Json(valor)).into_response()
}

/// Respuesta de error con la forma del contrato:
/// `{ "codigo": "...", "errores": [ { "campo": "...", "motivo": "..." } ] }`.
fn error(codigo: StatusCode, nombre: &str, errores: Vec<ErrorCampo>) -> Response {
    (codigo, Json(json!({ "codigo": nombre, "errores": errores }))).into_response()
}

/// El JSON no tiene la forma esperada → 422 con el motivo.
fn json_invalido(rechazo: JsonRejection) -> Response {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "solicitud_invalida",
        vec![ErrorCampo::new("cuerpo", &rechazo.body_text())],
    )
}

/// Traduce el error de un caso de uso a código HTTP y JSON.
fn traducir(e: ErrorCatalogo) -> Response {
    match e {
        ErrorCatalogo::Validacion(errores) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "validacion", errores)
        }
        ErrorCatalogo::NoEncontrado => error(StatusCode::NOT_FOUND, "no_encontrado", vec![]),
        ErrorCatalogo::YaExiste => error(
            StatusCode::CONFLICT,
            "producto_existente",
            vec![ErrorCampo::new(
                "slug",
                "ya existe un producto con ese slug; use otro o edítelo con PUT",
            )],
        ),
        ErrorCatalogo::ConflictoRevision => error(
            StatusCode::CONFLICT,
            "conflicto_edicion",
            vec![ErrorCampo::new(
                "documento",
                "alguien lo modificó al mismo tiempo; vuelva a cargarlo y reintente",
            )],
        ),
        ErrorCatalogo::NoDisponible(detalle) => {
            // El detalle técnico va al log del servidor, no al cliente.
            eprintln!("Productos/Config: base no disponible: {detalle}");
            error(StatusCode::SERVICE_UNAVAILABLE, "no_disponible", vec![])
        }
    }
}