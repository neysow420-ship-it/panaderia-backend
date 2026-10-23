//! # Dominio de hornadas
//!
//! Aquí están **los datos y las reglas** de una hornada, sin red ni base de datos.
//! Por eso se pueden probar solas con `cargo test`.
//!
//! Hay dos formas de una hornada:
//!
//! 1. [`SolicitudHornada`]: lo que **manda el cliente** (la PWA). Trae el ID de la
//!    operación, el producto, las unidades, la hora local y, opcionalmente, la fecha
//!    de vencimiento. **No trae usuario ni fecha de negocio**: los pone el servidor.
//! 2. [`Hornada`]: lo que **se guarda en CouchDB** (base `panaderia`). Sigue el
//!    formato de `contratos/documentos.md` §6.
//!
//! Relación con otras partes en este archivo:
//! - [`ProductoCatalogo`] lee el documento `producto` de la parte **Producto y Config**
//!   (base `catalogo`), pero solo los campos que hornada necesita.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ─────────────────────────────────────────────────────────────────────────────
// Errores de validación
// ─────────────────────────────────────────────────────────────────────────────

/// Un error de validación: **qué campo** falló y **por qué**.
///
/// Tiene la forma que pide el contrato de la API: `{ "campo": "...", "motivo": "..." }`.
/// La API devuelve una lista de estos dentro de `errores`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ErrorCampo {
    /// Nombre del campo con problema, ej. `operacion_id` o `unidades`.
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
// Relación con Producto y Config
// ─────────────────────────────────────────────────────────────────────────────

/// Producto del catálogo, **visto desde hornadas**.
///
/// Es la relación con la parte **Producto y Config**. En el documento estándar
/// el producto se ve así (base `catalogo`):
///
/// ```json
/// { "_id": "producto:pan-frances", "tipo": "producto", "nombre": "Pan francés",
///   "precio_centavos": 10, "vida_util_dias": 1, "activo": true }
/// ```
///
/// Hornada solo lee `_id` y `vida_util_dias` (para calcular el vencimiento por
/// defecto). Los demás campos se ignoran, así que si el compañero agrega campos
/// nuevos, hornada sigue funcionando.
///
/// El contrato (§6) pide que el producto **exista**; no pide que esté activo
/// (`activo` solo impide *vender*), por eso aquí no se lee.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ProductoCatalogo {
    /// `_id` del documento, ej. `producto:pan-frances`.
    #[serde(rename = "_id")]
    pub id: String,
    /// Días de vida útil del producto (1–365 en el catálogo).
    pub vida_util_dias: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Límites y zona horaria
// ─────────────────────────────────────────────────────────────────────────────

/// Mínimo de unidades por hornada.
pub const MIN_UNIDADES: i64 = 1;
/// Máximo de unidades por hornada.
pub const MAX_UNIDADES: i64 = 100_000;
/// Cuánto puede ir adelantado el reloj del dispositivo antes de rechazar la hornada (5 minutos).
pub const TOLERANCIA_FUTURO_SEG: i64 = 5 * 60;

/// Zona horaria del negocio: America/El_Salvador.
///
/// El Salvador está siempre en UTC-6 y no tiene horario de verano, por eso basta
/// un desfase fijo de 6 horas al oeste.
pub fn zona_negocio() -> FixedOffset {
    FixedOffset::west_opt(6 * 3600).expect("offset válido")
}

// ─────────────────────────────────────────────────────────────────────────────
// Lo que manda el cliente
// ─────────────────────────────────────────────────────────────────────────────

/// Cuerpo JSON de `POST /hornadas` (`SolicitudHornada` en `openapi.yaml`).
///
/// ```json
/// { "operacion_id": "01J9ZK5A2B3C4D5E6F7G8H9J0K", "producto_id": "producto:pan-frances",
///   "unidades": 150, "creado_local": "2026-10-13T05:30:00-06:00",
///   "fecha_vencimiento": "2026-10-14" }
/// ```
///
/// El contrato es **cerrado** (`additionalProperties: false`): si el cliente manda
/// `usuario`, `fecha`, `huella` u otro campo, se rechaza con 422. Esos datos los
/// calcula el servidor.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SolicitudHornada {
    /// ULID generado en el cliente **al abrir el formulario** y reutilizado en todos
    /// los reintentos. Es la **llave de idempotencia**.
    pub operacion_id: String,
    /// `_id` del producto en `catalogo`, ej. `producto:pan-frances`.
    pub producto_id: String,
    /// Unidades producidas (de [`MIN_UNIDADES`] a [`MAX_UNIDADES`]).
    pub unidades: i64,
    /// Hora del dispositivo en RFC 3339 **con zona**. Con esta hora se calcula el
    /// día de negocio de la hornada.
    pub creado_local: String,
    /// Opcional (`AAAA-MM-DD`). Si no viene, el servidor usa `fecha + vida_util_dias`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fecha_vencimiento: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Lo que se guarda en CouchDB
// ─────────────────────────────────────────────────────────────────────────────

/// Documento `hornada` en la base `panaderia` (`contratos/documentos.md` §6).
///
/// Los nombres de los campos en el JSON son exactamente los de este struct, salvo
/// `_id` y `_rev`. Es un documento de **evento**: el contrato es cerrado y no se
/// conservan campos ajenos.
///
/// También lo lee el módulo `vencimientos` (con Mango, por `tipo: "hornada"`): usa
/// `_id`, `producto_id`, `unidades` y `fecha_vencimiento`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Hornada {
    /// `_id` en CouchDB: `hornada:{ULID}`. Como el ULID viene del cliente, un
    /// reenvío choca con el mismo `_id` y CouchDB no deja duplicarla.
    #[serde(rename = "_id")]
    pub id: String,
    /// Revisión interna de CouchDB. Solo viene cuando el documento se lee de la base;
    /// al crear no se manda (por eso `skip_serializing_if`).
    #[serde(rename = "_rev", default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Siempre `"hornada"`. Sirve para filtrar en vistas e índices de `panaderia`,
    /// donde también hay ventas, descartes y cierres.
    pub tipo: String,
    /// `sha256:<hex>` del contenido que mandó el cliente. Ver [`SolicitudHornada::huella`].
    pub huella: String,
    /// Quién registró la hornada (lo pone el middleware de autenticación).
    pub usuario: String,
    /// Hora del dispositivo al crear la hornada, tal como llegó (RFC 3339 con zona).
    pub creado_en: String,
    /// **Día de negocio** de `creado_en` (`AAAA-MM-DD`). El cierre suma las hornadas
    /// por esta fecha para calcular `producidas`.
    pub fecha: NaiveDate,
    /// Relación con el producto: `_id` del documento en `catalogo`.
    pub producto_id: String,
    /// Unidades producidas.
    pub unidades: i64,
    /// Último día de vida útil del lote (`AAAA-MM-DD`), siempre ≥ `fecha`.
    pub fecha_vencimiento: NaiveDate,
    /// Reservado para planificación (el contrato lo marca como opcional). Hoy la API
    /// no lo acepta en la solicitud; el campo existe para leer y devolver documentos
    /// que ya lo traigan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pronostico_id: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Reglas de la solicitud
// ─────────────────────────────────────────────────────────────────────────────

/// Resultado de validar la forma de una solicitud: los valores ya interpretados,
/// para no tener que leerlos otra vez.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolicitudValidada {
    /// `creado_local` ya convertido a fecha y hora con zona.
    pub creado: DateTime<FixedOffset>,
    /// `fecha_vencimiento` ya convertida, si el cliente la mandó.
    pub fecha_vencimiento: Option<NaiveDate>,
}

impl SolicitudHornada {
    /// `_id` que tendrá el documento en CouchDB: `hornada:{ULID}`.
    pub fn id_documento(&self) -> String {
        format!("hornada:{}", self.operacion_id)
    }

    /// Revisa que la solicitud tenga la forma correcta. **No** consulta el catálogo
    /// (eso se hace en [`construir_hornada`]).
    ///
    /// Junta **todos** los errores en una lista en vez de parar en el primero,
    /// para que la pantalla de rechazadas los muestre de una vez.
    pub fn validar(&self) -> Result<SolicitudValidada, Vec<ErrorCampo>> {
        let mut errores = Vec::new();

        // El ID debe ser un ULID válido (lo genera el cliente).
        if !es_ulid(&self.operacion_id) {
            errores.push(ErrorCampo::new(
                "operacion_id",
                "debe ser un ULID de 26 caracteres",
            ));
        }

        // El producto debe tener la forma `producto:{slug}` (contrato §1.2).
        match self.producto_id.strip_prefix("producto:") {
            Some(slug) if es_slug(slug) => {}
            _ => errores.push(ErrorCampo::new(
                "producto_id",
                "debe tener la forma producto:<slug>, con slug en minúsculas, números y guiones",
            )),
        }

        if self.unidades < MIN_UNIDADES || self.unidades > MAX_UNIDADES {
            errores.push(ErrorCampo::new(
                "unidades",
                &format!("debe estar entre {MIN_UNIDADES} y {MAX_UNIDADES}"),
            ));
        }

        // La hora debe traer zona (ej. -06:00); si no, no se sabe qué día es.
        let creado = DateTime::parse_from_rfc3339(&self.creado_local);
        if creado.is_err() {
            errores.push(ErrorCampo::new(
                "creado_local",
                "debe ser fecha y hora RFC 3339 con zona, ej. 2026-10-13T05:30:00-06:00",
            ));
        }

        // El vencimiento es opcional, pero si viene debe ser una fecha AAAA-MM-DD.
        let vencimiento = match &self.fecha_vencimiento {
            None => None,
            Some(texto) => match NaiveDate::parse_from_str(texto, "%Y-%m-%d") {
                Ok(f) => Some(f),
                Err(_) => {
                    errores.push(ErrorCampo::new(
                        "fecha_vencimiento",
                        "debe tener el formato AAAA-MM-DD",
                    ));
                    None
                }
            },
        };

        // Regla del contrato: `fecha_vencimiento` ≥ `fecha` (día de negocio de la hornada).
        let vence_antes = match (&creado, vencimiento) {
            (Ok(creado), Some(venc)) => venc < fecha_negocio(creado),
            _ => false,
        };
        if vence_antes {
            errores.push(ErrorCampo::new(
                "fecha_vencimiento",
                "no puede ser anterior a la fecha de la hornada",
            ));
        }

        // Solo es válida si no hubo errores Y la fecha se pudo leer.
        match (errores.is_empty(), creado) {
            (true, Ok(creado)) => Ok(SolicitudValidada {
                creado,
                fecha_vencimiento: vencimiento,
            }),
            _ => Err(errores),
        }
    }

    /// Calcula la **huella** del contenido que mandó el cliente: `sha256:<hex>`.
    ///
    /// ¿Para qué sirve? Cuando llega una hornada con un ULID que ya existe:
    /// - Misma huella → es un reenvío de la misma hornada (se responde 200).
    /// - Huella distinta → alguien reusó el ID con otra hornada (se responde 409).
    ///
    /// Cómo se arma (texto fijo, `v1` es la versión del formato):
    ///
    /// ```text
    /// v1|<operacion_id>|<producto_id>|<unidades>|<creado_local>|<fecha_vencimiento>
    /// ```
    ///
    /// `fecha_vencimiento` va **tal como la mandó el cliente** y queda vacía si no la
    /// mandó. No se usa el valor calculado por el servidor, porque depende de
    /// `vida_util_dias` y podría cambiar entre un reintento y otro sin que el
    /// cliente haya decidido nada distinto. Tampoco incluye el usuario.
    ///
    /// > `contratos/documentos.md` §1.6 define la fórmula solo para ventas; esta es
    /// > la propuesta para hornadas y conviene documentarla allí.
    pub fn huella(&self) -> String {
        let canonico = format!(
            "v1|{}|{}|{}|{}|{}",
            self.operacion_id,
            self.producto_id,
            self.unidades,
            self.creado_local,
            self.fecha_vencimiento.as_deref().unwrap_or("")
        );

        let digest = Sha256::digest(canonico.as_bytes());
        // Cada byte se escribe como 2 dígitos hexadecimales.
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("sha256:{hex}")
    }
}

/// Día de negocio en El Salvador para una hora con cualquier zona.
///
/// Ej.: `2026-10-14T01:30:00Z` (UTC) son las 19:30 del 13 en El Salvador → día 13.
pub fn fecha_negocio(momento: &DateTime<FixedOffset>) -> NaiveDate {
    momento.with_timezone(&zona_negocio()).date_naive()
}

/// Arma la [`Hornada`] oficial a partir de la solicitud ya validada y el producto.
///
/// 1. `fecha` = día de negocio de `creado_local`.
/// 2. `fecha_vencimiento` = la que mandó el cliente o, si no vino,
///    `fecha + vida_util_dias` del producto (relación con **Producto y Config**).
/// 3. `usuario` lo pone el servidor.
///
/// Si el vencimiento calculado quedara antes de `fecha` (catálogo con datos
/// inválidos) o no se pudiera calcular, es un error y no un documento incoherente.
pub fn construir_hornada(
    solicitud: &SolicitudHornada,
    validada: &SolicitudValidada,
    producto: &ProductoCatalogo,
    usuario: &str,
) -> Result<Hornada, Vec<ErrorCampo>> {
    let fecha = fecha_negocio(&validada.creado);

    let fecha_vencimiento = match validada.fecha_vencimiento {
        Some(f) => f,
        None => match fecha.checked_add_signed(Duration::days(producto.vida_util_dias)) {
            Some(f) if f >= fecha => f,
            _ => {
                return Err(vec![ErrorCampo::new(
                    "fecha_vencimiento",
                    "no se pudo calcular desde la vida útil del producto",
                )]);
            }
        },
    };

    Ok(Hornada {
        id: solicitud.id_documento(),
        rev: None, // CouchDB la asigna al guardar.
        tipo: "hornada".to_string(),
        huella: solicitud.huella(),
        usuario: usuario.to_string(),
        creado_en: solicitud.creado_local.clone(),
        fecha,
        producto_id: solicitud.producto_id.clone(),
        unidades: solicitud.unidades,
        fecha_vencimiento,
        pronostico_id: None,
    })
}

/// Revisa si un texto es un ULID.
///
/// Un ULID tiene 26 caracteres en base32 de Crockford (números y letras
/// mayúsculas sin I, L, O ni U). El primer carácter va de 0 a 7 porque
/// los primeros bits son la hora.
pub fn es_ulid(texto: &str) -> bool {
    const ALFABETO: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    texto.len() == 26
        && texto.chars().all(|c| ALFABETO.contains(c))
        && texto.starts_with(|c: char| ('0'..='7').contains(&c))
}

/// Revisa si un texto es un slug válido: minúsculas, números y guiones, de 1 a 64
/// caracteres (contrato §1.2).
fn es_slug(texto: &str) -> bool {
    !texto.is_empty()
        && texto.len() <= 64
        && texto
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas unitarias del dominio (se corren con `cargo test`)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod pruebas {
    use super::*;

    fn producto() -> ProductoCatalogo {
        ProductoCatalogo {
            id: "producto:pan-frances".into(),
            vida_util_dias: 1,
        }
    }

    /// Mismo ejemplo del documento estándar (§6).
    fn solicitud() -> SolicitudHornada {
        SolicitudHornada {
            operacion_id: "01J9ZK5A2B3C4D5E6F7G8H9J0K".into(),
            producto_id: "producto:pan-frances".into(),
            unidades: 150,
            creado_local: "2026-10-13T05:30:00-06:00".into(),
            fecha_vencimiento: Some("2026-10-14".into()),
        }
    }

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    /// Una solicitud bien formada no da errores.
    #[test]
    fn solicitud_valida_pasa() {
        let v = solicitud().validar().unwrap();
        assert_eq!(v.fecha_vencimiento, Some(dia(14)));
    }

    /// El ejemplo del documento estándar se reproduce campo por campo.
    #[test]
    fn construye_el_ejemplo_del_documento() {
        let s = solicitud();
        let v = s.validar().unwrap();
        let h = construir_hornada(&s, &v, &producto(), "encargado").unwrap();
        assert_eq!(h.id, "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K");
        assert_eq!(h.tipo, "hornada");
        assert_eq!(h.usuario, "encargado");
        assert_eq!(h.creado_en, "2026-10-13T05:30:00-06:00");
        assert_eq!(h.fecha, dia(13));
        assert_eq!(h.producto_id, "producto:pan-frances");
        assert_eq!(h.unidades, 150);
        assert_eq!(h.fecha_vencimiento, dia(14));
    }

    /// Sin `fecha_vencimiento`, se usa `fecha + vida_util_dias`.
    #[test]
    fn vencimiento_por_defecto_usa_la_vida_util() {
        let mut s = solicitud();
        s.fecha_vencimiento = None;
        let v = s.validar().unwrap();

        let h = construir_hornada(&s, &v, &producto(), "encargado").unwrap();
        assert_eq!(h.fecha_vencimiento, dia(14)); // vida útil 1

        let semita = ProductoCatalogo {
            id: "producto:semita".into(),
            vida_util_dias: 2,
        };
        let h = construir_hornada(&s, &v, &semita, "encargado").unwrap();
        assert_eq!(h.fecha_vencimiento, dia(15)); // vida útil 2
    }

    /// Ejemplo inválido del contrato (§6.1): unidades 0 y vencimiento anterior a la fecha.
    #[test]
    fn rechaza_unidades_cero_y_vencimiento_anterior() {
        let mut s = solicitud();
        s.unidades = 0;
        s.fecha_vencimiento = Some("2026-10-12".into());
        let errores = s.validar().unwrap_err();
        let campos: Vec<&str> = errores.iter().map(|e| e.campo.as_str()).collect();
        assert!(campos.contains(&"unidades"));
        assert!(campos.contains(&"fecha_vencimiento"));
    }

    /// Límites de unidades: 1 y 100000 valen; 0 y 100001 no.
    #[test]
    fn limites_de_unidades() {
        for (unidades, ok) in [(1, true), (100_000, true), (0, false), (100_001, false)] {
            let mut s = solicitud();
            s.unidades = unidades;
            assert_eq!(s.validar().is_ok(), ok, "unidades = {unidades}");
        }
    }

    /// El vencimiento puede ser el mismo día de la hornada (≥ fecha).
    #[test]
    fn vencimiento_el_mismo_dia_es_valido() {
        let mut s = solicitud();
        s.fecha_vencimiento = Some("2026-10-13".into());
        assert!(s.validar().is_ok());
    }

    /// ID que no es ULID, producto mal escrito y hora sin zona dan un error cada uno.
    #[test]
    fn rechaza_ulid_producto_y_hora_invalidos() {
        let mut s = solicitud();
        s.operacion_id = "abc".into();
        s.producto_id = "pan-frances".into();
        s.creado_local = "2026-10-13 05:30:00".into();
        let errores = s.validar().unwrap_err();
        let campos: Vec<&str> = errores.iter().map(|e| e.campo.as_str()).collect();
        assert_eq!(campos, ["operacion_id", "producto_id", "creado_local"]);
    }

    /// Un slug con mayúsculas, espacios o barras no es un `producto_id` válido.
    #[test]
    fn rechaza_slugs_invalidos() {
        for malo in [
            "producto:",
            "producto:Pan",
            "producto:pan frances",
            "producto:a/b",
        ] {
            let mut s = solicitud();
            s.producto_id = malo.into();
            assert!(s.validar().is_err(), "{malo} debía rechazarse");
        }
    }

    /// Una fecha de vencimiento que no es AAAA-MM-DD se rechaza.
    #[test]
    fn rechaza_formato_de_vencimiento_invalido() {
        let mut s = solicitud();
        s.fecha_vencimiento = Some("14/10/2026".into());
        let errores = s.validar().unwrap_err();
        assert_eq!(errores.len(), 1);
        assert_eq!(errores[0].campo, "fecha_vencimiento");
    }

    /// Una hora en UTC se convierte al día correcto de El Salvador.
    #[test]
    fn fecha_negocio_usa_hora_de_el_salvador() {
        // 01:30 UTC del 14 son las 19:30 del 13 en El Salvador.
        let m = DateTime::parse_from_rfc3339("2026-10-14T01:30:00Z").unwrap();
        assert_eq!(fecha_negocio(&m), dia(13));
    }

    /// Una vida útil inválida en el catálogo no genera un vencimiento anterior a la fecha.
    #[test]
    fn vida_util_negativa_en_catalogo_es_error() {
        let mut s = solicitud();
        s.fecha_vencimiento = None;
        let v = s.validar().unwrap();
        let roto = ProductoCatalogo {
            id: "producto:pan-frances".into(),
            vida_util_dias: -3,
        };
        assert!(construir_hornada(&s, &v, &roto, "encargado").is_err());
    }

    /// Mismo contenido → misma huella; cambia cualquier campo → cambia la huella.
    #[test]
    fn huella_cambia_si_cambia_el_contenido() {
        let base = solicitud();
        assert_eq!(base.huella(), solicitud().huella());
        assert!(base.huella().starts_with("sha256:"));

        let mut otra = solicitud();
        otra.unidades = 151;
        assert_ne!(base.huella(), otra.huella());

        let mut otra = solicitud();
        otra.producto_id = "producto:donas".into();
        assert_ne!(base.huella(), otra.huella());

        let mut otra = solicitud();
        otra.creado_local = "2026-10-13T05:31:00-06:00".into();
        assert_ne!(base.huella(), otra.huella());

        let mut otra = solicitud();
        otra.fecha_vencimiento = None;
        assert_ne!(base.huella(), otra.huella());
    }

    /// El JSON que se guarda tiene los nombres del documento estándar.
    #[test]
    fn hornada_serializa_como_el_documento_estandar() {
        let s = solicitud();
        let v = s.validar().unwrap();
        let h = construir_hornada(&s, &v, &producto(), "encargado").unwrap();
        let json = serde_json::to_value(&h).unwrap();
        assert_eq!(json["_id"], "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K");
        assert_eq!(json["tipo"], "hornada");
        assert_eq!(json["fecha"], "2026-10-13");
        assert_eq!(json["fecha_vencimiento"], "2026-10-14");
        assert_eq!(json["unidades"], 150);
        // Sin _rev ni pronostico_id cuando no aplican.
        assert!(json.get("_rev").is_none());
        assert!(json.get("pronostico_id").is_none());
    }

    /// Un documento con `pronostico_id` (como el ejemplo del contrato) se lee y se conserva.
    #[test]
    fn lee_documento_con_pronostico_id() {
        let json = serde_json::json!({
            "_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K",
            "_rev": "1-2b4e",
            "tipo": "hornada",
            "huella": "sha256:abc",
            "usuario": "encargado",
            "creado_en": "2026-10-13T05:30:00-06:00",
            "fecha": "2026-10-13",
            "producto_id": "producto:pan-frances",
            "unidades": 150,
            "fecha_vencimiento": "2026-10-14",
            "pronostico_id": "pronostico:2026-10-13"
        });
        let h: Hornada = serde_json::from_value(json).unwrap();
        assert_eq!(h.pronostico_id.as_deref(), Some("pronostico:2026-10-13"));
        assert_eq!(h.rev.as_deref(), Some("1-2b4e"));
    }

    /// El contrato es cerrado: un campo que el cliente no debe mandar da error al leer el JSON.
    #[test]
    fn la_solicitud_rechaza_campos_desconocidos() {
        let json = serde_json::json!({
            "operacion_id": "01J9ZK5A2B3C4D5E6F7G8H9J0K",
            "producto_id": "producto:pan-frances",
            "unidades": 150,
            "creado_local": "2026-10-13T05:30:00-06:00",
            "usuario": "encargado"
        });
        assert!(serde_json::from_value::<SolicitudHornada>(json).is_err());
    }
}
