//! # Dominio de Producto y Config
//!
//! Aquí están **los datos y las reglas** de dos documentos del documento estándar,
//! sin red ni base de datos. Por eso se pueden probar solas con `cargo test`.
//!
//! 1. [`Producto`]: documento `producto:{slug}` en la base `catalogo`
//!    (nombre, precio en centavos, vida útil, si está activo).
//! 2. [`ConfigVigente`]: documento `config:vigente` en la base `panaderia`
//!    (desde cuándo rige, modo de producción, días de alerta, costos por producto).
//!
//! Lo que manda el cliente son [`SolicitudCrearProducto`], [`DatosProducto`] y
//! [`SolicitudConfig`]: traen solo los datos editables. `_id`, `_rev` y `tipo`
//! los pone el servidor.
//!
//! **Campos de otros compañeros**: el documento estándar dice que `producto` se
//! "extiende" (otras partes pueden agregarle campos). Por eso ambos structs guardan
//! en `otros` todo campo que no conocen, y al actualizar se vuelven a escribir, así
//! una edición de precio no borra lo que otro agregó.
//!
//! Relaciones con otras partes:
//! - **Ventas** lee `catalogo/producto:*` (`precio_centavos`, `activo`).
//! - **Horneada, Descarte, Cierre y Vencimientos** apuntan al producto por `producto_id`.
//! - **Cierre** usa `costos` y `modo_produccion` de `config:vigente` para la merma.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ─────────────────────────────────────────────────────────────────────────────
// Errores de validación
// ─────────────────────────────────────────────────────────────────────────────

/// Un error de validación: **qué campo** falló y **por qué**.
/// Forma del contrato de la API: `{ "campo": "...", "motivo": "..." }`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ErrorCampo {
    /// Nombre del campo con problema, ej. `slug` o `costos[0].producto_id`.
    pub campo: String,
    /// Explicación en español para mostrar al usuario.
    pub motivo: String,
}

impl ErrorCampo {
    /// Atajo para crear un error sin escribir `.to_string()` cada vez.
    pub fn new(campo: &str, motivo: &str) -> Self {
        Self {
            campo: campo.to_string(),
            motivo: motivo.to_string(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Límites y constantes
// ─────────────────────────────────────────────────────────────────────────────

/// Prefijo del `_id` de todo producto: `producto:pan-frances`.
pub const PREFIJO_PRODUCTO: &str = "producto:";
/// `_id` fijo del documento de configuración (solo hay uno vigente).
pub const ID_CONFIG: &str = "config:vigente";
/// Máximo de caracteres del nombre.
pub const MAX_NOMBRE: usize = 100;
/// Máximo de caracteres del slug (la parte del `_id` después de `producto:`).
pub const MAX_SLUG: usize = 64;
/// Precio o costo máximo en centavos (1_000_000 = $10,000). Evita errores de dedo.
pub const MAX_PRECIO_CENTAVOS: i64 = 1_000_000;
/// Vida útil máxima en días.
pub const MAX_VIDA_UTIL_DIAS: i64 = 365;
/// Máximo de días de anticipación de la alerta de vencimiento.
pub const MAX_ALERTA_DIAS: i64 = 30;
/// Máximo de caracteres del modo de producción.
pub const MAX_MODO: usize = 32;
/// Máximo de costos en la configuración.
pub const MAX_COSTOS: usize = 500;

/// Valor por defecto de `activo` cuando no viene.
fn verdadero() -> bool {
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Producto
// ─────────────────────────────────────────────────────────────────────────────

/// Documento `producto` en la base `catalogo`. Formato del documento estándar:
///
/// ```json
/// { "_id": "producto:pan-frances", "tipo": "producto", "nombre": "Pan francés",
///   "precio_centavos": 10, "vida_util_dias": 1, "activo": true }
/// ```
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Producto {
    /// `_id` en CouchDB: `producto:{slug}`. Es el `producto_id` que usan las demás partes.
    #[serde(rename = "_id")]
    pub id: String,
    /// Revisión interna de CouchDB. Solo viene al leer; al crear no se manda.
    #[serde(rename = "_rev", default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Siempre `"producto"`. Sirve para filtrar en `catalogo`.
    pub tipo: String,
    /// Nombre para mostrar.
    pub nombre: String,
    /// Precio de venta en centavos (10 = $0.10). Nunca decimales con dinero.
    pub precio_centavos: i64,
    /// Días que dura el producto después de hornearse (lo usa Vencimientos).
    pub vida_util_dias: i64,
    /// Si es `false`, ya no se vende. No se borran productos: Ventas, Cierre y
    /// Descarte los siguen referenciando en documentos históricos.
    #[serde(default = "verdadero")]
    pub activo: bool,
    /// Campos que agregó otra parte y que aquí no se conocen. Se conservan.
    #[serde(flatten)]
    pub otros: Map<String, Value>,
}

/// Datos editables de un producto (lo que manda el cliente al editar).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DatosProducto {
    /// Nombre para mostrar, ej. `Pan francés`.
    pub nombre: String,
    /// Precio de venta en centavos.
    pub precio_centavos: i64,
    /// Vida útil en días.
    pub vida_util_dias: i64,
    /// Si se vende o no. Si no viene, se toma como `true`.
    #[serde(default = "verdadero")]
    pub activo: bool,
}

/// Cuerpo JSON de `POST /productos`: los datos más el `slug` que formará el `_id`.
///
/// ```json
/// { "slug": "pan-frances", "nombre": "Pan francés", "precio_centavos": 10,
///   "vida_util_dias": 1, "activo": true }
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct SolicitudCrearProducto {
    /// Identificador corto: minúsculas, números y guiones (`pan-frances`).
    pub slug: String,
    /// El resto de los campos, en el mismo nivel del JSON.
    #[serde(flatten)]
    pub datos: DatosProducto,
}

/// `_id` del documento a partir del slug: `pan-frances` → `producto:pan-frances`.
pub fn id_producto(slug: &str) -> String {
    format!("{PREFIJO_PRODUCTO}{slug}")
}

/// ¿El texto sirve como slug? Minúsculas ASCII, números y guiones; no empieza
/// ni termina en guion; de 1 a [`MAX_SLUG`] caracteres.
pub fn es_slug(texto: &str) -> bool {
    !texto.is_empty()
        && texto.len() <= MAX_SLUG
        && texto
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !texto.starts_with('-')
        && !texto.ends_with('-')
}

/// Error del slug, o `None` si está bien.
pub fn validar_slug(slug: &str) -> Option<ErrorCampo> {
    if es_slug(slug) {
        None
    } else {
        Some(ErrorCampo::new(
            "slug",
            &format!(
                "use minúsculas, números y guiones (sin tildes ni espacios), máximo {MAX_SLUG}"
            ),
        ))
    }
}

impl DatosProducto {
    /// Revisa nombre, precio y vida útil. Junta **todos** los errores en una lista.
    pub fn validar(&self) -> Vec<ErrorCampo> {
        let mut errores = Vec::new();

        let nombre = self.nombre.trim();
        if nombre.is_empty() || nombre.chars().count() > MAX_NOMBRE {
            errores.push(ErrorCampo::new(
                "nombre",
                &format!("obligatorio, máximo {MAX_NOMBRE} caracteres"),
            ));
        }
        if !(1..=MAX_PRECIO_CENTAVOS).contains(&self.precio_centavos) {
            errores.push(ErrorCampo::new(
                "precio_centavos",
                &format!("debe estar entre 1 y {MAX_PRECIO_CENTAVOS} centavos"),
            ));
        }
        if !(1..=MAX_VIDA_UTIL_DIAS).contains(&self.vida_util_dias) {
            errores.push(ErrorCampo::new(
                "vida_util_dias",
                &format!("debe estar entre 1 y {MAX_VIDA_UTIL_DIAS} días"),
            ));
        }
        errores
    }
}

/// Arma el [`Producto`] oficial. `rev` y `otros` vienen del documento existente
/// al actualizar (para no perder la revisión ni los campos de otras partes);
/// al crear van vacíos.
pub fn construir_producto(
    slug: &str,
    datos: &DatosProducto,
    rev: Option<String>,
    otros: Map<String, Value>,
) -> Producto {
    Producto {
        id: id_producto(slug),
        rev,
        tipo: "producto".to_string(),
        nombre: datos.nombre.trim().to_string(),
        precio_centavos: datos.precio_centavos,
        vida_util_dias: datos.vida_util_dias,
        activo: datos.activo,
        otros,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config
// ─────────────────────────────────────────────────────────────────────────────

/// Costo de producir una unidad de un producto (lo usa Cierre para calcular la merma).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CostoProducto {
    /// `_id` del producto en `catalogo`, ej. `producto:pan-frances`.
    pub producto_id: String,
    /// Costo por unidad en centavos.
    pub costo_unitario_centavos: i64,
}

/// Documento `config:vigente` en la base `panaderia`:
///
/// ```json
/// { "_id": "config:vigente", "tipo": "config", "vigente_desde": "2026-10-13",
///   "modo_produccion": "modelo", "alerta_dias": 1,
///   "costos": [ { "producto_id": "producto:pan-frances", "costo_unitario_centavos": 5 } ] }
/// ```
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConfigVigente {
    /// Siempre `config:vigente`.
    #[serde(rename = "_id")]
    pub id: String,
    /// Revisión interna de CouchDB.
    #[serde(rename = "_rev", default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Siempre `"config"`.
    pub tipo: String,
    /// Día desde el que rige esta configuración (`AAAA-MM-DD`).
    pub vigente_desde: NaiveDate,
    /// Cómo se decide cuánto producir (hoy el estándar solo muestra `"modelo"`).
    pub modo_produccion: String,
    /// Con cuántos días de anticipación se avisa que un lote va a vencer.
    pub alerta_dias: i64,
    /// Costo unitario por producto.
    pub costos: Vec<CostoProducto>,
    /// Campos que agregó otra parte y que aquí no se conocen. Se conservan.
    #[serde(flatten)]
    pub otros: Map<String, Value>,
}

/// Cuerpo JSON de `PUT /config`: todo lo editable de la configuración.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SolicitudConfig {
    /// Día desde el que rige.
    pub vigente_desde: NaiveDate,
    /// Modo de producción.
    pub modo_produccion: String,
    /// Días de anticipación de la alerta de vencimiento.
    pub alerta_dias: i64,
    /// Costos por producto.
    pub costos: Vec<CostoProducto>,
}

impl SolicitudConfig {
    /// Revisa la forma de la configuración. **No** consulta el catálogo
    /// (que cada `producto_id` exista se revisa en el caso de uso).
    pub fn validar(&self) -> Vec<ErrorCampo> {
        let mut errores = Vec::new();

        let modo = self.modo_produccion.trim();
        if modo.is_empty() || modo.chars().count() > MAX_MODO {
            errores.push(ErrorCampo::new(
                "modo_produccion",
                &format!("obligatorio, máximo {MAX_MODO} caracteres"),
            ));
        }
        if !(0..=MAX_ALERTA_DIAS).contains(&self.alerta_dias) {
            errores.push(ErrorCampo::new(
                "alerta_dias",
                &format!("debe estar entre 0 y {MAX_ALERTA_DIAS}"),
            ));
        }
        if self.costos.len() > MAX_COSTOS {
            errores.push(ErrorCampo::new(
                "costos",
                &format!("máximo {MAX_COSTOS} costos"),
            ));
        }

        // `vistos` detecta productos repetidos en la lista de costos.
        let mut vistos = HashSet::new();
        for (i, c) in self.costos.iter().enumerate() {
            if !c.producto_id.starts_with(PREFIJO_PRODUCTO) {
                errores.push(ErrorCampo::new(
                    &format!("costos[{i}].producto_id"),
                    "debe tener la forma producto:<nombre>",
                ));
            } else if !vistos.insert(c.producto_id.as_str()) {
                errores.push(ErrorCampo::new(
                    &format!("costos[{i}].producto_id"),
                    "producto repetido; deje un solo costo por producto",
                ));
            }
            if !(0..=MAX_PRECIO_CENTAVOS).contains(&c.costo_unitario_centavos) {
                errores.push(ErrorCampo::new(
                    &format!("costos[{i}].costo_unitario_centavos"),
                    &format!("debe estar entre 0 y {MAX_PRECIO_CENTAVOS} centavos"),
                ));
            }
        }
        errores
    }
}

/// Arma el [`ConfigVigente`] oficial. `rev` y `otros` vienen del documento
/// existente (si lo hay).
pub fn construir_config(
    solicitud: &SolicitudConfig,
    rev: Option<String>,
    otros: Map<String, Value>,
) -> ConfigVigente {
    ConfigVigente {
        id: ID_CONFIG.to_string(),
        rev,
        tipo: "config".to_string(),
        vigente_desde: solicitud.vigente_desde,
        modo_produccion: solicitud.modo_produccion.trim().to_string(),
        alerta_dias: solicitud.alerta_dias,
        costos: solicitud.costos.clone(),
        otros,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas unitarias del dominio (se corren con `cargo test`)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod pruebas {
    use super::*;
    use serde_json::json;

    fn datos() -> DatosProducto {
        DatosProducto {
            nombre: "Pan francés".into(),
            precio_centavos: 10,
            vida_util_dias: 1,
            activo: true,
        }
    }

    fn config() -> SolicitudConfig {
        SolicitudConfig {
            vigente_desde: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            modo_produccion: "modelo".into(),
            alerta_dias: 1,
            costos: vec![CostoProducto {
                producto_id: "producto:pan-frances".into(),
                costo_unitario_centavos: 5,
            }],
        }
    }

    /// Slugs bien y mal formados.
    #[test]
    fn valida_slugs() {
        assert!(es_slug("pan-frances"));
        assert!(es_slug("donas2"));
        assert!(!es_slug(""));
        assert!(!es_slug("Pan"));
        assert!(!es_slug("pan frances"));
        assert!(!es_slug("-pan"));
        assert!(!es_slug("pan-"));
        assert!(!es_slug("pan/frances"));
        assert!(validar_slug("pan francés").is_some());
    }

    /// Datos correctos no dan errores; incorrectos dan uno por campo.
    #[test]
    fn valida_datos_de_producto() {
        assert!(datos().validar().is_empty());
        let mal = DatosProducto {
            nombre: "   ".into(),
            precio_centavos: 0,
            vida_util_dias: 400,
            activo: true,
        };
        let campos: Vec<String> = mal.validar().into_iter().map(|e| e.campo).collect();
        assert_eq!(campos, ["nombre", "precio_centavos", "vida_util_dias"]);
    }

    /// El producto se guarda con los nombres del documento estándar.
    #[test]
    fn producto_serializa_como_el_documento_estandar() {
        let p = construir_producto("pan-frances", &datos(), None, Map::new());
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["_id"], "producto:pan-frances");
        assert_eq!(j["tipo"], "producto");
        assert_eq!(j["precio_centavos"], 10);
        assert_eq!(j["activo"], true);
        assert!(j.get("_rev").is_none());
    }

    /// Un campo que no conocemos (de otra parte) se conserva al leer y volver a escribir.
    #[test]
    fn conserva_campos_de_otras_partes() {
        let original = json!({
            "_id": "producto:donas", "_rev": "1-abc", "tipo": "producto",
            "nombre": "Donas", "precio_centavos": 35, "vida_util_dias": 2,
            "activo": true, "categoria": "dulces"
        });
        let p: Producto = serde_json::from_value(original).unwrap();
        assert_eq!(p.otros["categoria"], "dulces");
        let de_vuelta = serde_json::to_value(&p).unwrap();
        assert_eq!(de_vuelta["categoria"], "dulces");
        assert_eq!(de_vuelta["_rev"], "1-abc");
    }

    /// `POST /productos` trae el slug y los datos en el mismo nivel.
    #[test]
    fn solicitud_de_creacion_lee_slug_y_datos() {
        let s: SolicitudCrearProducto = serde_json::from_value(json!({
            "slug": "donas", "nombre": "Donas", "precio_centavos": 35, "vida_util_dias": 2
        }))
        .unwrap();
        assert_eq!(s.slug, "donas");
        assert!(s.datos.activo); // por defecto
    }

    /// Config correcta pasa; producto repetido, alerta fuera de rango y costo negativo fallan.
    #[test]
    fn valida_config() {
        assert!(config().validar().is_empty());
        let mut mala = config();
        mala.alerta_dias = 99;
        mala.costos.push(mala.costos[0].clone());
        mala.costos[0].costo_unitario_centavos = -1;
        let campos: Vec<String> = mala.validar().into_iter().map(|e| e.campo).collect();
        assert!(campos.contains(&"alerta_dias".to_string()));
        assert!(campos.contains(&"costos[1].producto_id".to_string()));
        assert!(campos.contains(&"costos[0].costo_unitario_centavos".to_string()));
    }

    /// La config se guarda con los nombres del documento estándar.
    #[test]
    fn config_serializa_como_el_documento_estandar() {
        let c = construir_config(&config(), None, Map::new());
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["_id"], "config:vigente");
        assert_eq!(j["tipo"], "config");
        assert_eq!(j["vigente_desde"], "2026-10-13");
        assert_eq!(j["costos"][0]["costo_unitario_centavos"], 5);
    }
}