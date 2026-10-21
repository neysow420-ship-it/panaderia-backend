//! # Casos de uso de Producto y Config
//!
//! Es el **paso a paso** de cada operación. Usa las reglas del
//! [`dominio`](super::dominio) y le pide los datos a un [`RepositorioProductos`]
//! (CouchDB en producción, memoria en las pruebas).
//!
//! | Caso de uso            | Qué hace |
//! |------------------------|----------|
//! | [`listar_productos`]   | Lista el catálogo (opcional: solo activos), ordenado por nombre |
//! | [`obtener_producto`]   | Lee un producto por slug |
//! | [`crear_producto`]     | Valida y crea; si el slug ya existe → `YaExiste` (409) |
//! | [`actualizar_producto`]| Valida, toma `_rev` y campos ajenos del existente y reemplaza |
//! | [`obtener_config`]     | Lee `config:vigente` |
//! | [`guardar_config`]     | Valida (incluye que los productos existan) y crea o reemplaza |
//!
//! **Choques de edición**: CouchDB responde 409 si alguien modificó el documento
//! entre que se leyó y se guardó (la `_rev` ya no coincide). Eso es
//! `ConflictoRevision`: el cliente debe volver a cargar y reintentar.

use std::collections::HashSet;
use std::future::Future;

use serde_json::Map;

use super::dominio::{
    ConfigVigente, DatosProducto, ErrorCampo, Producto, SolicitudConfig,
    SolicitudCrearProducto, construir_config, construir_producto, id_producto, validar_slug,
};

// ─────────────────────────────────────────────────────────────────────────────
// Contrato con la base de datos
// ─────────────────────────────────────────────────────────────────────────────

/// Errores que puede dar la base y que le importan a los casos de uso.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorRepositorio {
    /// CouchDB respondió 409: ya existe el `_id` (al crear) o la `_rev` quedó vieja (al actualizar).
    Conflicto,
    /// CouchDB no responde o respondió algo inesperado. El texto es para el log.
    NoDisponible(String),
}

/// **Qué necesitan los casos de uso de la base**, sin decir cómo se hace.
///
/// Es un *trait* (como una interfaz en Java). Lo implementan `CouchDb` en
/// `couchdb.rs` (programa real) y `Memoria` en las pruebas de abajo.
pub trait RepositorioProductos {
    /// Busca un producto por `_id`. `Ok(None)` si no existe.
    fn obtener_producto(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<Producto>, ErrorRepositorio>> + Send;
    /// Todos los productos de `catalogo`.
    fn listar_productos(
        &self,
    ) -> impl Future<Output = Result<Vec<Producto>, ErrorRepositorio>> + Send;
    /// Crea el producto. Si el `_id` ya existe devuelve `Err(Conflicto)`.
    fn crear_producto(
        &self,
        producto: &Producto,
    ) -> impl Future<Output = Result<(), ErrorRepositorio>> + Send;
    /// Reemplaza el producto (trae `rev`). Si la `rev` quedó vieja devuelve `Err(Conflicto)`.
    fn actualizar_producto(
        &self,
        producto: &Producto,
    ) -> impl Future<Output = Result<(), ErrorRepositorio>> + Send;
    /// Lee `config:vigente`. `Ok(None)` si todavía no existe.
    fn obtener_config(
        &self,
    ) -> impl Future<Output = Result<Option<ConfigVigente>, ErrorRepositorio>> + Send;
    /// Crea o reemplaza `config:vigente` (con `rev` si ya existía).
    fn guardar_config(
        &self,
        config: &ConfigVigente,
    ) -> impl Future<Output = Result<(), ErrorRepositorio>> + Send;
}

// ─────────────────────────────────────────────────────────────────────────────
// Errores de los casos de uso
// ─────────────────────────────────────────────────────────────────────────────

/// Resultado cuando algo falla. La API lo traduce a un código HTTP.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorCatalogo {
    /// Datos inválidos, con la lista de errores por campo → **422**.
    Validacion(Vec<ErrorCampo>),
    /// No existe el producto o la configuración pedida → **404**.
    NoEncontrado,
    /// Se intentó crear un producto cuyo slug ya existe → **409**.
    YaExiste,
    /// Alguien modificó el documento al mismo tiempo (`_rev` vieja) → **409**.
    ConflictoRevision,
    /// CouchDB no responde → **503**.
    NoDisponible(String),
}

/// Permite usar `?` con errores del repositorio. Un `Conflicto` genérico se toma
/// como choque de revisión; `crear_producto` lo cambia a `YaExiste` a mano.
impl From<ErrorRepositorio> for ErrorCatalogo {
    fn from(e: ErrorRepositorio) -> Self {
        match e {
            ErrorRepositorio::Conflicto => ErrorCatalogo::ConflictoRevision,
            ErrorRepositorio::NoDisponible(m) => ErrorCatalogo::NoDisponible(m),
        }
    }
}

/// Valida el slug y lo convierte en error de caso de uso si está mal.
fn revisar_slug(slug: &str) -> Result<(), ErrorCatalogo> {
    match validar_slug(slug) {
        None => Ok(()),
        Some(e) => Err(ErrorCatalogo::Validacion(vec![e])),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Productos
// ─────────────────────────────────────────────────────────────────────────────

/// Lista el catálogo ordenado por nombre. Con `solo_activos` quita los inactivos.
pub async fn listar_productos<R: RepositorioProductos>(
    repo: &R,
    solo_activos: bool,
) -> Result<Vec<Producto>, ErrorCatalogo> {
    let mut productos = repo.listar_productos().await?;
    if solo_activos {
        productos.retain(|p| p.activo);
    }
    productos.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    Ok(productos)
}

/// Lee un producto por slug (`pan-frances`). 404 si no existe.
pub async fn obtener_producto<R: RepositorioProductos>(
    repo: &R,
    slug: &str,
) -> Result<Producto, ErrorCatalogo> {
    revisar_slug(slug)?;
    repo.obtener_producto(&id_producto(slug))
        .await?
        .ok_or(ErrorCatalogo::NoEncontrado)
}

/// Crea un producto nuevo.
///
/// 1. Valida slug y datos (junta todos los errores).
/// 2. Arma el documento y lo guarda con `PUT`. Si CouchDB dice 409, ya existía.
pub async fn crear_producto<R: RepositorioProductos>(
    repo: &R,
    solicitud: &SolicitudCrearProducto,
) -> Result<Producto, ErrorCatalogo> {
    let mut errores: Vec<ErrorCampo> = validar_slug(&solicitud.slug).into_iter().collect();
    errores.extend(solicitud.datos.validar());
    if !errores.is_empty() {
        return Err(ErrorCatalogo::Validacion(errores));
    }

    let producto = construir_producto(&solicitud.slug, &solicitud.datos, None, Map::new());
    match repo.crear_producto(&producto).await {
        Ok(()) => Ok(producto),
        Err(ErrorRepositorio::Conflicto) => Err(ErrorCatalogo::YaExiste),
        Err(e) => Err(e.into()),
    }
}

/// Reemplaza los datos de un producto existente.
///
/// 1. Valida slug y datos.
/// 2. Lee el producto actual (404 si no existe) para tomar su `_rev` y los
///    campos que haya agregado otra parte.
/// 3. Guarda. Si la `_rev` ya cambió, CouchDB responde 409 → `ConflictoRevision`.
pub async fn actualizar_producto<R: RepositorioProductos>(
    repo: &R,
    slug: &str,
    datos: &DatosProducto,
) -> Result<Producto, ErrorCatalogo> {
    let mut errores: Vec<ErrorCampo> = validar_slug(slug).into_iter().collect();
    errores.extend(datos.validar());
    if !errores.is_empty() {
        return Err(ErrorCatalogo::Validacion(errores));
    }

    let existente = repo
        .obtener_producto(&id_producto(slug))
        .await?
        .ok_or(ErrorCatalogo::NoEncontrado)?;
    let nuevo = construir_producto(slug, datos, existente.rev, existente.otros);
    repo.actualizar_producto(&nuevo).await?;
    Ok(nuevo)
}

// ─────────────────────────────────────────────────────────────────────────────
// Config
// ─────────────────────────────────────────────────────────────────────────────

/// Lee `config:vigente`. 404 si todavía no se ha creado.
pub async fn obtener_config<R: RepositorioProductos>(
    repo: &R,
) -> Result<ConfigVigente, ErrorCatalogo> {
    repo.obtener_config()
        .await?
        .ok_or(ErrorCatalogo::NoEncontrado)
}

/// Crea o reemplaza la configuración vigente.
///
/// 1. Valida la forma.
/// 2. Revisa que cada `producto_id` de `costos` exista en el catálogo
///    (si no, Cierre calcularía merma de un producto fantasma).
/// 3. Toma `_rev` y campos ajenos del documento actual (si hay) y guarda.
pub async fn guardar_config<R: RepositorioProductos>(
    repo: &R,
    solicitud: &SolicitudConfig,
) -> Result<ConfigVigente, ErrorCatalogo> {
    let mut errores = solicitud.validar();

    if errores.is_empty() {
        let existentes: HashSet<String> = repo
            .listar_productos()
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        for (i, c) in solicitud.costos.iter().enumerate() {
            if !existentes.contains(&c.producto_id) {
                errores.push(ErrorCampo::new(
                    &format!("costos[{i}].producto_id"),
                    "producto no existe en el catálogo",
                ));
            }
        }
    }
    if !errores.is_empty() {
        return Err(ErrorCatalogo::Validacion(errores));
    }

    let (rev, otros) = match repo.obtener_config().await? {
        Some(actual) => (actual.rev, actual.otros),
        None => (None, Map::new()),
    };
    let config = construir_config(solicitud, rev, otros);
    repo.guardar_config(&config).await?;
    Ok(config)
}

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas de los casos de uso, con una base en memoria en lugar de CouchDB
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod pruebas {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use chrono::NaiveDate;
    use serde_json::json;

    use super::*;
    use crate::productos::dominio::CostoProducto;

    /// Repositorio en memoria que imita a CouchDB (implementa el mismo trait).
    #[derive(Default)]
    struct Memoria {
        productos: Mutex<HashMap<String, Producto>>,
        config: Mutex<Option<ConfigVigente>>,
    }

    impl RepositorioProductos for Memoria {
        async fn obtener_producto(&self, id: &str) -> Result<Option<Producto>, ErrorRepositorio> {
            Ok(self.productos.lock().unwrap().get(id).cloned())
        }
        async fn listar_productos(&self) -> Result<Vec<Producto>, ErrorRepositorio> {
            Ok(self.productos.lock().unwrap().values().cloned().collect())
        }
        async fn crear_producto(&self, p: &Producto) -> Result<(), ErrorRepositorio> {
            let mut m = self.productos.lock().unwrap();
            if m.contains_key(&p.id) {
                return Err(ErrorRepositorio::Conflicto);
            }
            m.insert(p.id.clone(), p.clone());
            Ok(())
        }
        async fn actualizar_producto(&self, p: &Producto) -> Result<(), ErrorRepositorio> {
            self.productos.lock().unwrap().insert(p.id.clone(), p.clone());
            Ok(())
        }
        async fn obtener_config(&self) -> Result<Option<ConfigVigente>, ErrorRepositorio> {
            Ok(self.config.lock().unwrap().clone())
        }
        async fn guardar_config(&self, c: &ConfigVigente) -> Result<(), ErrorRepositorio> {
            *self.config.lock().unwrap() = Some(c.clone());
            Ok(())
        }
    }

    fn datos(nombre: &str, precio: i64) -> DatosProducto {
        DatosProducto {
            nombre: nombre.into(),
            precio_centavos: precio,
            vida_util_dias: 1,
            activo: true,
        }
    }

    fn crear(slug: &str, nombre: &str, precio: i64) -> SolicitudCrearProducto {
        SolicitudCrearProducto {
            slug: slug.into(),
            datos: datos(nombre, precio),
        }
    }

    fn solicitud_config(producto_id: &str) -> SolicitudConfig {
        SolicitudConfig {
            vigente_desde: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            modo_produccion: "modelo".into(),
            alerta_dias: 1,
            costos: vec![CostoProducto {
                producto_id: producto_id.into(),
                costo_unitario_centavos: 5,
            }],
        }
    }

    /// Producto nuevo: se guarda y se puede leer.
    #[tokio::test]
    async fn crea_y_lee_un_producto() {
        let repo = Memoria::default();
        let p = crear_producto(&repo, &crear("pan-frances", "Pan francés", 10))
            .await
            .unwrap();
        assert_eq!(p.id, "producto:pan-frances");
        let leido = obtener_producto(&repo, "pan-frances").await.unwrap();
        assert_eq!(leido.precio_centavos, 10);
    }

    /// Mismo slug dos veces: YaExiste (409).
    #[tokio::test]
    async fn slug_repetido_es_ya_existe() {
        let repo = Memoria::default();
        crear_producto(&repo, &crear("donas", "Donas", 35)).await.unwrap();
        let r = crear_producto(&repo, &crear("donas", "Donas 2", 40)).await;
        assert_eq!(r, Err(ErrorCatalogo::YaExiste));
    }

    /// Datos malos: 422 con todos los errores juntos.
    #[tokio::test]
    async fn datos_invalidos_son_validacion() {
        let repo = Memoria::default();
        let r = crear_producto(&repo, &crear("Pan Malo", "", 0)).await;
        let Err(ErrorCatalogo::Validacion(errores)) = r else {
            panic!("debía fallar la validación")
        };
        assert_eq!(errores.len(), 3);
    }

    /// Actualizar cambia el precio y conserva `_rev` y los campos de otras partes.
    #[tokio::test]
    async fn actualizar_conserva_rev_y_campos_ajenos() {
        let repo = Memoria::default();
        let mut otros = Map::new();
        otros.insert("categoria".into(), json!("dulces"));
        repo.productos.lock().unwrap().insert(
            "producto:donas".into(),
            Producto {
                id: "producto:donas".into(),
                rev: Some("1-abc".into()),
                tipo: "producto".into(),
                nombre: "Donas".into(),
                precio_centavos: 35,
                vida_util_dias: 2,
                activo: true,
                otros,
            },
        );
        let nuevo = actualizar_producto(&repo, "donas", &datos("Donas", 40))
            .await
            .unwrap();
        assert_eq!(nuevo.precio_centavos, 40);
        assert_eq!(nuevo.rev.as_deref(), Some("1-abc"));
        assert_eq!(nuevo.otros["categoria"], "dulces");
    }

    /// Actualizar algo que no existe: NoEncontrado (404).
    #[tokio::test]
    async fn actualizar_inexistente_es_404() {
        let repo = Memoria::default();
        let r = actualizar_producto(&repo, "fantasma", &datos("X", 10)).await;
        assert_eq!(r, Err(ErrorCatalogo::NoEncontrado));
    }

    /// El listado sale ordenado por nombre y puede filtrar inactivos.
    #[tokio::test]
    async fn lista_ordenado_y_filtra_inactivos() {
        let repo = Memoria::default();
        crear_producto(&repo, &crear("donas", "Donas", 35)).await.unwrap();
        crear_producto(&repo, &crear("pan-frances", "Pan francés", 10))
            .await
            .unwrap();
        let mut viejo = crear("viejo", "Antiguo", 50);
        viejo.datos.activo = false;
        crear_producto(&repo, &viejo).await.unwrap();

        let todos = listar_productos(&repo, false).await.unwrap();
        let nombres: Vec<&str> = todos.iter().map(|p| p.nombre.as_str()).collect();
        assert_eq!(nombres, ["Antiguo", "Donas", "Pan francés"]);
        assert_eq!(listar_productos(&repo, true).await.unwrap().len(), 2);
    }

    /// Sin config guardada: NoEncontrado.
    #[tokio::test]
    async fn config_sin_crear_es_404() {
        let repo = Memoria::default();
        assert_eq!(obtener_config(&repo).await, Err(ErrorCatalogo::NoEncontrado));
    }

    /// Un costo de un producto que no existe en el catálogo se rechaza.
    #[tokio::test]
    async fn config_con_producto_inexistente_es_422() {
        let repo = Memoria::default();
        let r = guardar_config(&repo, &solicitud_config("producto:fantasma")).await;
        assert!(matches!(r, Err(ErrorCatalogo::Validacion(_))));
    }

    /// Config correcta: se guarda y se puede leer; guardar otra vez la reemplaza.
    #[tokio::test]
    async fn guarda_y_reemplaza_config() {
        let repo = Memoria::default();
        crear_producto(&repo, &crear("pan-frances", "Pan francés", 10))
            .await
            .unwrap();
        let c = guardar_config(&repo, &solicitud_config("producto:pan-frances"))
            .await
            .unwrap();
        assert_eq!(c.id, "config:vigente");

        let mut otra = solicitud_config("producto:pan-frances");
        otra.alerta_dias = 2;
        guardar_config(&repo, &otra).await.unwrap();
        assert_eq!(obtener_config(&repo).await.unwrap().alerta_dias, 2);
    }
}