//! # Dominio de ventas
//!
//! Aquí están **los datos y las reglas** de una venta, sin red ni base de datos.
//! Por eso se pueden probar solas con `cargo test`.
//!
//! Hay dos formas de una venta:
//!
//! 1. [`SolicitudVenta`]: lo que **manda el cliente** (la PWA). Solo trae el ID,
//!    la caja, la hora local y qué productos y cuántos. **No trae precios**.
//! 2. [`Venta`]: lo que **se guarda en CouchDB** (base `panaderia`). Rust le pone
//!    los precios del catálogo, los subtotales, el total, la huella, el usuario
//!    y la fecha de negocio. Sigue el formato del documento estándar del grupo.
//!
//! Relación con otras partes en este archivo:
//! - [`ProductoCatalogo`] lee el documento `producto` de la parte **Producto y Config**
//!   (base `catalogo`), pero solo los campos que venta necesita.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, FixedOffset, NaiveDate};
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
    /// Nombre del campo con problema, ej. `operacion_id` o `lineas[0].cantidad`.
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

/// Producto del catálogo, **visto desde ventas**.
///
/// Es la relación con la parte **Producto y Config**. En el documento estándar
/// el producto se ve así (base `catalogo`):
///
/// ```json
/// { "_id": "producto:pan-frances", "tipo": "producto", "nombre": "Pan francés",
///   "precio_centavos": 10, "vida_util_dias": 1, "activo": true }
/// ```
///
/// Venta solo lee `_id`, `nombre`, `precio_centavos` y `activo`. Los demás campos
/// (`vida_util_dias`, etc.) se ignoran, así que si el compañero agrega campos
/// nuevos, venta sigue funcionando.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ProductoCatalogo {
    /// `_id` del documento, ej. `producto:pan-frances`. Es lo que guarda cada línea
    /// de la venta en `producto_id`.
    #[serde(rename = "_id")]
    pub id: String,
    /// Nombre para mostrar.
    pub nombre: String,
    /// Precio de venta en centavos (10 = $0.10). Nunca se usan decimales con dinero.
    pub precio_centavos: i64,
    /// Si es `false`, el producto ya no se vende. Si el documento no trae el campo,
    /// se toma como `true`.
    #[serde(default = "verdadero")]
    pub activo: bool,
}

/// Valor por defecto de `activo` cuando el documento no lo trae.
fn verdadero() -> bool {
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Límites y zona horaria
// ─────────────────────────────────────────────────────────────────────────────

/// Máximo de líneas por venta (evita peticiones gigantes).
pub const MAX_LINEAS: usize = 50;
/// Máximo de unidades de un producto en una sola línea.
pub const MAX_CANTIDAD: i64 = 1000;
/// Cuánto puede ir adelantado el reloj de la caja antes de rechazar la venta (5 minutos).
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

/// Una línea **tal como la manda el cliente**: solo producto y cantidad.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LineaSolicitud {
    /// `_id` del producto en `catalogo`, ej. `producto:donas`.
    pub producto_id: String,
    /// Unidades vendidas (de 1 a [`MAX_CANTIDAD`]).
    pub cantidad: i64,
}

/// Cuerpo JSON de `POST /ventas`.
///
/// ```json
/// { "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C1D", "dispositivo_id": "caja-1",
///   "creado_local": "2026-10-13T08:15:32-06:00",
///   "lineas": [ { "producto_id": "producto:pan-frances", "cantidad": 6 } ] }
/// ```
///
/// El cliente **no manda precios ni totales**: si los mandara, alguien podría
/// cambiarlos. Rust los calcula con el catálogo.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SolicitudVenta {
    /// ULID generado en el cliente cuando se hizo la venta. Es la **llave de
    /// idempotencia**: si la misma venta llega dos veces, trae el mismo ULID.
    pub operacion_id: String,
    /// Qué caja o dispositivo hizo la venta, ej. `caja-1`.
    pub dispositivo_id: String,
    /// Hora del dispositivo en RFC 3339 **con zona**, ej. `2026-10-13T08:15:32-06:00`.
    /// Con esta hora se calcula a qué día pertenece la venta.
    pub creado_local: String,
    /// Productos vendidos.
    pub lineas: Vec<LineaSolicitud>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Lo que se guarda en CouchDB
// ─────────────────────────────────────────────────────────────────────────────

/// Línea **oficial** de la venta, con el precio del catálogo **congelado**.
///
/// Si mañana sube el precio del pan, esta venta conserva el precio con el que se vendió.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LineaVenta {
    /// Relación con el producto: `_id` del documento en `catalogo`.
    pub producto_id: String,
    /// Unidades vendidas.
    pub cantidad: i64,
    /// Precio unitario en centavos en el momento de la venta.
    pub precio_centavos: i64,
    /// `cantidad × precio_centavos`.
    pub subtotal_centavos: i64,
}

/// Documento `venta` en la base `panaderia`.
///
/// Es el mismo formato del documento estándar del grupo, más `fecha_original`
/// (opcional, solo para ventas de un día ya cerrado). Los nombres de los campos
/// en el JSON son exactamente los de este struct, salvo `_id` y `_rev`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Venta {
    /// `_id` en CouchDB: `venta:{ULID}`. Como el ULID viene del cliente, un
    /// reenvío choca con el mismo `_id` y CouchDB no deja duplicarla.
    #[serde(rename = "_id")]
    pub id: String,
    /// Revisión interna de CouchDB. Solo viene cuando el documento se lee de la base;
    /// al crear no se manda (por eso `skip_serializing_if`).
    #[serde(rename = "_rev", default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Siempre `"venta"`. Sirve para filtrar en vistas e índices de `panaderia`,
    /// donde también hay hornadas, descartes y cierres.
    pub tipo: String,
    /// El mismo ULID de la solicitud, sin el prefijo `venta:`.
    pub operacion_id: String,
    /// `sha256:<hex>` del contenido que mandó el cliente. Ver [`SolicitudVenta::huella`].
    pub huella: String,
    /// Quién registró la venta (lo pone el middleware de autenticación).
    pub usuario: String,
    /// Caja que hizo la venta.
    pub dispositivo_id: String,
    /// Hora del dispositivo, tal como llegó.
    pub creado_local: String,
    /// Hora del servidor cuando aceptó la venta (sin fracciones de segundo).
    pub recibido_servidor: String,
    /// **Día de negocio** al que cuenta la venta (`AAAA-MM-DD`). Es la fecha que
    /// usan el cierre y los reportes.
    pub fecha: NaiveDate,
    /// Solo en ventas tardías: el día real de la venta, si ese día ya tenía cierre.
    /// En ese caso `fecha` es el día abierto al que se movió.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fecha_original: Option<NaiveDate>,
    /// Líneas con precios oficiales.
    pub lineas: Vec<LineaVenta>,
    /// Suma de los subtotales, en centavos.
    pub total_centavos: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Reglas de la solicitud
// ─────────────────────────────────────────────────────────────────────────────

impl SolicitudVenta {
    /// `_id` que tendrá el documento en CouchDB: `venta:{ULID}`.
    pub fn id_documento(&self) -> String {
        format!("venta:{}", self.operacion_id)
    }

    /// Revisa que la solicitud tenga la forma correcta. **No** consulta el catálogo
    /// (eso se hace en [`construir_venta`]).
    ///
    /// Junta **todos** los errores en una lista en vez de parar en el primero,
    /// para que la pantalla de rechazadas los muestre de una vez.
    ///
    /// Si todo está bien, devuelve la hora `creado_local` ya convertida a fecha,
    /// para no tener que leerla otra vez.
    pub fn validar(&self) -> Result<DateTime<FixedOffset>, Vec<ErrorCampo>> {
        let mut errores = Vec::new();

        // El ID debe ser un ULID válido (lo genera el cliente).
        if !es_ulid(&self.operacion_id) {
            errores.push(ErrorCampo::new(
                "operacion_id",
                "debe ser un ULID de 26 caracteres",
            ));
        }

        // La caja es obligatoria; `trim` quita espacios al inicio y al final.
        let dispositivo = self.dispositivo_id.trim();
        if dispositivo.is_empty() || dispositivo.len() > 64 {
            errores.push(ErrorCampo::new(
                "dispositivo_id",
                "obligatorio, máximo 64 caracteres",
            ));
        }

        // La hora debe traer zona (ej. -06:00); si no, no se sabe qué día es.
        let creado = DateTime::parse_from_rfc3339(&self.creado_local);
        if creado.is_err() {
            errores.push(ErrorCampo::new(
                "creado_local",
                "debe ser fecha y hora RFC 3339 con zona, ej. 2026-10-13T08:15:32-06:00",
            ));
        }

        // Debe haber al menos una línea y no demasiadas.
        if self.lineas.is_empty() {
            errores.push(ErrorCampo::new("lineas", "la venta no tiene productos"));
        } else if self.lineas.len() > MAX_LINEAS {
            errores.push(ErrorCampo::new(
                "lineas",
                &format!("máximo {MAX_LINEAS} líneas"),
            ));
        }

        // Revisión línea por línea. `vistos` guarda los productos ya revisados
        // para detectar si uno viene repetido.
        let mut vistos = HashSet::new();
        for (i, linea) in self.lineas.iter().enumerate() {
            if !linea.producto_id.starts_with("producto:") {
                errores.push(ErrorCampo::new(
                    &format!("lineas[{i}].producto_id"),
                    "debe tener la forma producto:<nombre>",
                ));
            } else if !vistos.insert(linea.producto_id.as_str()) {
                // `insert` devuelve false si el producto ya estaba.
                errores.push(ErrorCampo::new(
                    &format!("lineas[{i}].producto_id"),
                    "producto repetido; sume las cantidades en una sola línea",
                ));
            }
            if linea.cantidad < 1 || linea.cantidad > MAX_CANTIDAD {
                errores.push(ErrorCampo::new(
                    &format!("lineas[{i}].cantidad"),
                    &format!("debe estar entre 1 y {MAX_CANTIDAD}"),
                ));
            }
        }

        // Solo es válida si no hubo errores Y la fecha se pudo leer.
        match (errores.is_empty(), creado) {
            (true, Ok(fecha)) => Ok(fecha),
            _ => Err(errores),
        }
    }

    /// Calcula la **huella** del contenido que mandó el cliente: `sha256:<hex>`.
    ///
    /// ¿Para qué sirve? Cuando llega una venta con un ULID que ya existe:
    /// - Misma huella → es un reenvío de la misma venta (se responde 200).
    /// - Huella distinta → alguien reusó el ID con otra venta (se responde 409).
    ///
    /// Cómo se arma:
    /// 1. Las líneas se ordenan por producto, así el orden no cambia la huella.
    /// 2. Se escribe un texto fijo: `v1|ULID|caja|hora|producto=cantidad;...`
    ///    (`v1` es la versión del formato por si algún día cambia).
    /// 3. Se le aplica SHA-256 y se pasa a hexadecimal.
    ///
    /// No incluye datos del servidor (usuario, hora de recepción, precios), porque
    /// esos pueden cambiar entre un envío y otro sin que la venta sea distinta.
    pub fn huella(&self) -> String {
        let mut lineas: Vec<&LineaSolicitud> = self.lineas.iter().collect();
        lineas.sort_by(|a, b| a.producto_id.cmp(&b.producto_id));

        let mut canonico = format!(
            "v1|{}|{}|{}|",
            self.operacion_id,
            self.dispositivo_id.trim(),
            self.creado_local
        );
        for l in lineas {
            canonico.push_str(&format!("{}={};", l.producto_id, l.cantidad));
        }

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

/// Datos que **pone el servidor** al aceptar una venta (no vienen del cliente).
pub struct DatosServidor<'a> {
    /// Usuario que dejó el middleware de autenticación.
    pub usuario: &'a str,
    /// Hora del servidor al recibir la venta.
    pub recibido: DateTime<FixedOffset>,
    /// Día de negocio ya calculado (puede ser el día abierto si el original estaba cerrado).
    pub fecha: NaiveDate,
    /// Día real, solo si se movió por estar cerrado.
    pub fecha_original: Option<NaiveDate>,
}

/// Arma la [`Venta`] oficial a partir de la solicitud y el catálogo.
///
/// Por cada línea:
/// 1. Busca el producto en el catálogo (relación con **Producto y Config**).
/// 2. Si no existe o está inactivo, anota el error.
/// 3. Si existe, calcula `subtotal = precio × cantidad` y lo suma al total.
///
/// Todo es en centavos (`i64`) y con `checked_mul` / `checked_add`: si un número
/// se desborda, es un error y no un total equivocado.
pub fn construir_venta(
    solicitud: &SolicitudVenta,
    catalogo: &HashMap<String, ProductoCatalogo>,
    servidor: DatosServidor<'_>,
) -> Result<Venta, Vec<ErrorCampo>> {
    let mut errores = Vec::new();
    let mut lineas = Vec::with_capacity(solicitud.lineas.len());
    let mut total: i64 = 0;

    for (i, l) in solicitud.lineas.iter().enumerate() {
        let campo = format!("lineas[{i}].producto_id");
        match catalogo.get(&l.producto_id) {
            // El producto no está en `catalogo`.
            None => errores.push(ErrorCampo::new(&campo, "producto no existe en el catálogo")),
            // Existe pero ya no se vende.
            Some(p) if !p.activo => errores.push(ErrorCampo::new(&campo, "producto inactivo")),
            // Existe y está activo: se calcula el importe.
            Some(p) => {
                // `checked_mul` devuelve None si el resultado no cabe en un i64.
                let subtotal = p.precio_centavos.checked_mul(l.cantidad);
                // Si el subtotal cabe, también se revisa que el total siga cabiendo.
                match subtotal.and_then(|s| total.checked_add(s).map(|t| (s, t))) {
                    Some((s, t)) => {
                        total = t;
                        lineas.push(LineaVenta {
                            producto_id: l.producto_id.clone(),
                            cantidad: l.cantidad,
                            precio_centavos: p.precio_centavos,
                            subtotal_centavos: s,
                        });
                    }
                    None => errores.push(ErrorCampo::new(
                        &format!("lineas[{i}].cantidad"),
                        "el importe es demasiado grande",
                    )),
                }
            }
        }
    }

    if !errores.is_empty() {
        return Err(errores);
    }

    // Se arma el documento con el formato del documento estándar.
    Ok(Venta {
        id: solicitud.id_documento(),
        rev: None, // CouchDB la asigna al guardar.
        tipo: "venta".to_string(),
        operacion_id: solicitud.operacion_id.clone(),
        huella: solicitud.huella(),
        usuario: servidor.usuario.to_string(),
        dispositivo_id: solicitud.dispositivo_id.trim().to_string(),
        creado_local: solicitud.creado_local.clone(),
        // Formato `2026-10-13T08:15:40-06:00`, igual que en el documento estándar.
        recibido_servidor: servidor
            .recibido
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        fecha: servidor.fecha,
        fecha_original: servidor.fecha_original,
        lineas,
        total_centavos: total,
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

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas unitarias del dominio (se corren con `cargo test`)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod pruebas {
    use super::*;

    fn catalogo() -> HashMap<String, ProductoCatalogo> {
        let mut c = HashMap::new();
        for (id, precio, activo) in [
            ("producto:pan-frances", 10, true),
            ("producto:donas", 35, true),
            ("producto:viejo", 50, false),
        ] {
            c.insert(
                id.to_string(),
                ProductoCatalogo {
                    id: id.to_string(),
                    nombre: id.to_string(),
                    precio_centavos: precio,
                    activo,
                },
            );
        }
        c
    }

    fn solicitud() -> SolicitudVenta {
        SolicitudVenta {
            operacion_id: "01J9ZK3M8Q4V7T2R5N6X8B0C1D".into(),
            dispositivo_id: "caja-1".into(),
            creado_local: "2026-10-13T08:15:32-06:00".into(),
            lineas: vec![
                LineaSolicitud {
                    producto_id: "producto:pan-frances".into(),
                    cantidad: 6,
                },
                LineaSolicitud {
                    producto_id: "producto:donas".into(),
                    cantidad: 2,
                },
            ],
        }
    }

    fn servidor() -> DatosServidor<'static> {
        DatosServidor {
            usuario: "cajero",
            recibido: DateTime::parse_from_rfc3339("2026-10-13T08:15:40-06:00").unwrap(),
            fecha: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            fecha_original: None,
        }
    }

    /// Una solicitud bien formada no da errores.

    #[test]
    fn solicitud_valida_pasa() {
        assert!(solicitud().validar().is_ok());
    }

    /// Mismo ejemplo del documento estándar: 6 panes × 10 + 2 donas × 35 = 130 centavos.

    #[test]
    fn calcula_total_del_ejemplo_del_documento() {
        let v = construir_venta(&solicitud(), &catalogo(), servidor()).unwrap();
        assert_eq!(v.id, "venta:01J9ZK3M8Q4V7T2R5N6X8B0C1D");
        assert_eq!(v.lineas[0].subtotal_centavos, 60);
        assert_eq!(v.lineas[1].subtotal_centavos, 70);
        assert_eq!(v.total_centavos, 130);
    }

    /// ID que no es ULID, cantidad 0 y venta sin líneas dan 422.

    #[test]
    fn rechaza_ulid_invalido_cantidad_y_lineas_vacias() {
        let mut s = solicitud();
        s.operacion_id = "abc".into();
        s.lineas[0].cantidad = 0;
        let errores = s.validar().unwrap_err();
        let campos: Vec<&str> = errores.iter().map(|e| e.campo.as_str()).collect();
        assert!(campos.contains(&"operacion_id"));
        assert!(campos.contains(&"lineas[0].cantidad"));

        s.lineas.clear();
        assert!(s.validar().is_err());
    }

    /// El mismo producto en dos líneas y una hora sin zona dan 2 errores.

    #[test]
    fn rechaza_producto_repetido_y_fecha_sin_zona() {
        let mut s = solicitud();
        s.lineas[1].producto_id = "producto:pan-frances".into();
        s.creado_local = "2026-10-13 08:15:32".into();
        let errores = s.validar().unwrap_err();
        assert_eq!(errores.len(), 2);
    }

    /// Relación con Producto: un producto que no está o está inactivo no se puede vender.

    #[test]
    fn rechaza_producto_inexistente_o_inactivo() {
        let mut s = solicitud();
        s.lineas[0].producto_id = "producto:no-existe".into();
        s.lineas[1].producto_id = "producto:viejo".into();
        let errores = construir_venta(&s, &catalogo(), servidor()).unwrap_err();
        assert_eq!(errores.len(), 2);
    }

    /// Las mismas líneas en otro orden dan la misma huella.

    #[test]
    fn huella_no_depende_del_orden_de_las_lineas() {
        let a = solicitud();
        let mut b = solicitud();
        b.lineas.reverse();
        assert_eq!(a.huella(), b.huella());
        assert!(a.huella().starts_with("sha256:"));
    }

    /// Si cambia una cantidad, la huella cambia (eso permite detectar el 409).

    #[test]
    fn huella_cambia_si_cambia_la_cantidad() {
        let a = solicitud();
        let mut b = solicitud();
        b.lineas[0].cantidad = 7;
        assert_ne!(a.huella(), b.huella());
    }

    /// Una hora en UTC se convierte al día correcto de El Salvador.

    #[test]
    fn fecha_negocio_usa_hora_de_el_salvador() {
        // 01:30 UTC del 14 son las 19:30 del 13 en El Salvador.
        let m = DateTime::parse_from_rfc3339("2026-10-14T01:30:00Z").unwrap();
        assert_eq!(
            fecha_negocio(&m),
            NaiveDate::from_ymd_opt(2026, 10, 13).unwrap()
        );
    }

    /// El JSON que se guarda tiene los nombres del documento estándar.

    #[test]
    fn venta_serializa_como_el_documento_estandar() {
        let v = construir_venta(&solicitud(), &catalogo(), servidor()).unwrap();
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["_id"], "venta:01J9ZK3M8Q4V7T2R5N6X8B0C1D");
        assert_eq!(json["tipo"], "venta");
        assert_eq!(json["fecha"], "2026-10-13");
        // Sin _rev ni fecha_original cuando no aplican.
        assert!(json.get("_rev").is_none());
        assert!(json.get("fecha_original").is_none());
    }
}
