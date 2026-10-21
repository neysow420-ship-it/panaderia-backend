//! # Caso de uso: registrar una venta
//!
//! Es el **paso a paso** de qué pasa cuando llega una venta. Usa las reglas del
//! [`dominio`](super::dominio) y le pide los datos a un [`RepositorioVentas`]
//! (CouchDB en producción, memoria en las pruebas).
//!
//! 1. **Validar** la solicitud (forma, ULID, hora, líneas).
//! 2. **Idempotencia**: si `venta:{ULID}` ya existe, comparar la huella:
//!    igual → `YaProcesada` (200); distinta → `ConflictoOperacion` (409).
//! 3. **Día de negocio**: se calcula con la hora local. Si ese día ya tiene
//!    `cierre:{fecha}` (relación con la parte **Cierre**), la venta pasa al
//!    primer día abierto y se guarda el día real en `fecha_original`.
//! 4. **Armar y guardar**: precios del catálogo, total y `PUT` en CouchDB.
//!    Solo se responde éxito cuando CouchDB confirmó la escritura.

use std::collections::HashMap;
use std::future::Future;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate};

use super::dominio::ErrorCampo;
use super::dominio::ProductoCatalogo;
use super::dominio::{
    DatosServidor, SolicitudVenta, TOLERANCIA_FUTURO_SEG, Venta, construir_venta, fecha_negocio,
};

// ─────────────────────────────────────────────────────────────────────────────
// Contrato con la base de datos
// ─────────────────────────────────────────────────────────────────────────────

/// Errores que puede dar la base y que le importan al caso de uso.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorRepositorio {
    /// CouchDB respondió 409: ya existe un documento con ese `_id`.
    Conflicto,
    /// CouchDB no responde o respondió algo inesperado. El texto es para el log.
    NoDisponible(String),
}

/// **Qué necesita el caso de uso de la base**, sin decir cómo se hace.
///
/// Es un *trait* (como una interfaz en Java). Lo implementan:
/// - `CouchDb` en `couchdb.rs`, para el programa real.
/// - `Memoria` en las pruebas de abajo, para probar sin CouchDB.
///
/// Así el caso de uso no sabe nada de HTTP ni de Reqwest.
///
/// (Se escribe `-> impl Future<...> + Send` en vez de `async fn` para que Axum
/// pueda usarlo entre hilos.)
pub trait RepositorioVentas {
    /// Busca una venta por su `_id`. `Ok(None)` si no existe.
    fn obtener_venta(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<Venta>, ErrorRepositorio>> + Send;
    /// Crea la venta. Si el `_id` ya existe devuelve `Err(Conflicto)`.
    fn crear_venta(
        &self,
        venta: &Venta,
    ) -> impl Future<Output = Result<(), ErrorRepositorio>> + Send;
    /// Relación con **Cierre**: ¿existe `cierre:{fecha}`, o sea, ese día ya se cerró?
    fn existe_cierre(
        &self,
        fecha: NaiveDate,
    ) -> impl Future<Output = Result<bool, ErrorRepositorio>> + Send;
}

// ─────────────────────────────────────────────────────────────────────────────
// Resultados
// ─────────────────────────────────────────────────────────────────────────────

/// Resultado cuando todo sale bien. La API lo traduce a un código HTTP.
#[derive(Debug, PartialEq, Eq)]
pub enum ResultadoVenta {
    /// La venta era nueva y se guardó → **201**.
    Creada(Venta),
    /// La venta ya estaba guardada con el mismo contenido (reenvío) → **200**.
    YaProcesada(Venta),
}

/// Resultado cuando algo falla. La API lo traduce a un código HTTP.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorVenta {
    /// Datos inválidos, con la lista de errores por campo → **422**.
    Validacion(Vec<ErrorCampo>),
    /// Ya existe una venta con ese ULID pero con otro contenido → **409**.
    ConflictoOperacion,
    /// CouchDB no responde → **503** (el cliente reintenta más tarde).
    NoDisponible(String),
}

/// Permite usar `?` con errores del repositorio dentro del caso de uso:
/// convierte automáticamente un `ErrorRepositorio` en `ErrorVenta`.
impl From<ErrorRepositorio> for ErrorVenta {
    fn from(e: ErrorRepositorio) -> Self {
        match e {
            ErrorRepositorio::Conflicto => ErrorVenta::ConflictoOperacion,
            ErrorRepositorio::NoDisponible(m) => ErrorVenta::NoDisponible(m),
        }
    }
}

/// Máximo de días que se avanza buscando un día sin cierre
/// (protección para no quedarse en un ciclo infinito).
const MAX_DIAS_ADELANTE: i64 = 7;

// ─────────────────────────────────────────────────────────────────────────────
// Caso de uso
// ─────────────────────────────────────────────────────────────────────────────

/// Registra una venta siguiendo los 4 pasos del inicio de este archivo.
///
/// - `repo`: acceso a la base (CouchDB o memoria).
/// - `catalogo`: productos cargados al iniciar (relación con **Producto y Config**).
/// - `solicitud`: lo que mandó el cliente.
/// - `usuario`: quién vende (relación con **Login**, viene del middleware).
/// - `ahora`: hora del servidor. Se pasa como parámetro para poder probar con
///   cualquier fecha.
pub async fn registrar_venta<R: RepositorioVentas>(
    repo: &R,
    catalogo: &HashMap<String, ProductoCatalogo>,
    solicitud: &SolicitudVenta,
    usuario: &str,
    ahora: DateTime<FixedOffset>,
) -> Result<ResultadoVenta, ErrorVenta> {
    // ── Paso 1: validar la forma de la solicitud ──
    let creado = solicitud.validar().map_err(ErrorVenta::Validacion)?;
    // Una venta "del futuro" significa que el reloj de la caja está mal.
    if creado > ahora + Duration::seconds(TOLERANCIA_FUTURO_SEG) {
        return Err(ErrorVenta::Validacion(vec![ErrorCampo::new(
            "creado_local",
            "la hora está en el futuro; revise el reloj de la caja",
        )]));
    }

    // ── Paso 2: idempotencia, ¿ya llegó esta venta antes? ──
    let huella = solicitud.huella();
    if let Some(existente) = repo.obtener_venta(&solicitud.id_documento()).await? {
        return comparar_con_existente(existente, &huella);
    }

    // ── Paso 3: día de negocio y regla de día cerrado (relación con Cierre) ──
    let original = fecha_negocio(&creado); // día en que se hizo la venta
    let hoy = fecha_negocio(&ahora); // día en el servidor
    let (fecha, fecha_original) = if repo.existe_cierre(original).await? {
        // El día original ya se cerró: se busca el primer día abierto desde hoy
        // (o desde el día original si fuera posterior) y se recuerda el real.
        (dia_abierto(repo, original.max(hoy)).await?, Some(original))
    } else {
        (original, None)
    };

    // ── Paso 4: precios oficiales y guardado ──
    let venta = construir_venta(
        solicitud,
        catalogo,
        DatosServidor {
            usuario,
            recibido: ahora,
            fecha,
            fecha_original,
        },
    )
    .map_err(ErrorVenta::Validacion)?;

    match repo.crear_venta(&venta).await {
        // CouchDB confirmó: ahora sí se responde éxito.
        Ok(()) => Ok(ResultadoVenta::Creada(venta)),
        // Dos envíos de la misma venta llegaron casi al mismo tiempo y el otro
        // la guardó primero. Se lee la guardada y se compara la huella.
        Err(ErrorRepositorio::Conflicto) => match repo.obtener_venta(&venta.id).await? {
            Some(existente) => comparar_con_existente(existente, &huella),
            None => Err(ErrorVenta::NoDisponible(
                "conflicto sin documento".to_string(),
            )),
        },
        // Cualquier otro error de la base.
        Err(e) => Err(e.into()),
    }
}

/// Compara una venta ya guardada con la huella de la que acaba de llegar.
/// Misma huella → reenvío (200). Distinta → conflicto (409).
fn comparar_con_existente(existente: Venta, huella: &str) -> Result<ResultadoVenta, ErrorVenta> {
    if existente.huella == huella {
        Ok(ResultadoVenta::YaProcesada(existente))
    } else {
        Err(ErrorVenta::ConflictoOperacion)
    }
}

/// Devuelve el primer día **sin cierre** a partir de `desde`, revisando
/// hasta [`MAX_DIAS_ADELANTE`] días.
async fn dia_abierto<R: RepositorioVentas>(
    repo: &R,
    desde: NaiveDate,
) -> Result<NaiveDate, ErrorVenta> {
    let mut dia = desde;
    for _ in 0..MAX_DIAS_ADELANTE {
        if !repo.existe_cierre(dia).await? {
            return Ok(dia);
        }
        dia += Duration::days(1);
    }
    Err(ErrorVenta::Validacion(vec![ErrorCampo::new(
        "creado_local",
        "no hay un día abierto cercano para asignar la venta",
    )]))
}

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas del caso de uso, con una base en memoria en lugar de CouchDB
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod pruebas {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use super::*;
    use crate::ventas::dominio::LineaSolicitud;

    /// Repositorio en memoria que imita a CouchDB (implementa el mismo trait).
    #[derive(Default)]
    struct Memoria {
        ventas: Mutex<HashMap<String, Venta>>,
        cierres: HashSet<NaiveDate>,
        /// Simula que otro envío guardó el documento justo antes que nosotros.
        carrera: Mutex<Option<Venta>>,
    }

    impl RepositorioVentas for Memoria {
        async fn obtener_venta(&self, id: &str) -> Result<Option<Venta>, ErrorRepositorio> {
            Ok(self.ventas.lock().unwrap().get(id).cloned())
        }
        async fn crear_venta(&self, venta: &Venta) -> Result<(), ErrorRepositorio> {
            if let Some(otra) = self.carrera.lock().unwrap().take() {
                self.ventas.lock().unwrap().insert(otra.id.clone(), otra);
            }
            let mut v = self.ventas.lock().unwrap();
            if v.contains_key(&venta.id) {
                return Err(ErrorRepositorio::Conflicto);
            }
            v.insert(venta.id.clone(), venta.clone());
            Ok(())
        }
        async fn existe_cierre(&self, fecha: NaiveDate) -> Result<bool, ErrorRepositorio> {
            Ok(self.cierres.contains(&fecha))
        }
    }

    fn catalogo() -> HashMap<String, ProductoCatalogo> {
        HashMap::from([(
            "producto:pan-frances".to_string(),
            ProductoCatalogo {
                id: "producto:pan-frances".into(),
                nombre: "Pan francés".into(),
                precio_centavos: 10,
                activo: true,
            },
        )])
    }

    fn solicitud(cantidad: i64, creado: &str) -> SolicitudVenta {
        SolicitudVenta {
            operacion_id: "01J9ZK3M8Q4V7T2R5N6X8B0C1D".into(),
            dispositivo_id: "caja-1".into(),
            creado_local: creado.into(),
            lineas: vec![LineaSolicitud {
                producto_id: "producto:pan-frances".into(),
                cantidad,
            }],
        }
    }

    fn ahora(texto: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(texto).unwrap()
    }

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    const CREADO: &str = "2026-10-13T08:15:32-06:00";
    const AHORA: &str = "2026-10-13T08:15:40-06:00";

    /// Venta nueva: se guarda y se responde Creada (201).

    #[tokio::test]
    async fn primera_vez_crea_la_venta() {
        let repo = Memoria::default();
        let r = registrar_venta(
            &repo,
            &catalogo(),
            &solicitud(6, CREADO),
            "cajero",
            ahora(AHORA),
        )
        .await
        .unwrap();
        let ResultadoVenta::Creada(v) = r else {
            panic!("debía crearse")
        };
        assert_eq!(v.total_centavos, 60);
        assert_eq!(v.fecha, dia(13));
        assert_eq!(repo.ventas.lock().unwrap().len(), 1);
    }

    /// Mismo ULID y mismo contenido: no se duplica y se responde YaProcesada (200).

    #[tokio::test]
    async fn reenvio_igual_devuelve_ya_procesada() {
        let repo = Memoria::default();
        let s = solicitud(6, CREADO);
        registrar_venta(&repo, &catalogo(), &s, "cajero", ahora(AHORA))
            .await
            .unwrap();
        let r = registrar_venta(&repo, &catalogo(), &s, "cajero", ahora(AHORA))
            .await
            .unwrap();
        assert!(matches!(r, ResultadoVenta::YaProcesada(_)));
        assert_eq!(repo.ventas.lock().unwrap().len(), 1);
    }

    /// Mismo ULID con otra cantidad: ConflictoOperacion (409).

    #[tokio::test]
    async fn mismo_id_con_otro_contenido_es_409() {
        let repo = Memoria::default();
        registrar_venta(
            &repo,
            &catalogo(),
            &solicitud(6, CREADO),
            "cajero",
            ahora(AHORA),
        )
        .await
        .unwrap();
        let r = registrar_venta(
            &repo,
            &catalogo(),
            &solicitud(7, CREADO),
            "cajero",
            ahora(AHORA),
        )
        .await;
        assert_eq!(r, Err(ErrorVenta::ConflictoOperacion));
    }

    /// Dos envíos iguales al mismo tiempo: el segundo recibe 409 de CouchDB, relee y responde 200.

    #[tokio::test]
    async fn carrera_entre_dos_envios_iguales_no_falla() {
        let repo = Memoria::default();
        let s = solicitud(6, CREADO);
        // Preparamos la "otra" copia guardada justo en medio.
        let previa =
            match registrar_venta(&Memoria::default(), &catalogo(), &s, "cajero", ahora(AHORA))
                .await
                .unwrap()
            {
                ResultadoVenta::Creada(v) => v,
                _ => unreachable!(),
            };
        *repo.carrera.lock().unwrap() = Some(previa);
        let r = registrar_venta(&repo, &catalogo(), &s, "cajero", ahora(AHORA))
            .await
            .unwrap();
        assert!(matches!(r, ResultadoVenta::YaProcesada(_)));
    }

    /// Relación con Cierre: venta offline del día 12 ya cerrado pasa al 13 con fecha_original = 12.

    #[tokio::test]
    async fn venta_de_dia_cerrado_pasa_al_dia_abierto() {
        let repo = Memoria {
            cierres: HashSet::from([dia(12)]),
            ..Default::default()
        };
        // Venta hecha offline el 12 a las 19:50, enviada el 13 en la mañana.
        let s = solicitud(6, "2026-10-12T19:50:00-06:00");
        let r = registrar_venta(&repo, &catalogo(), &s, "cajero", ahora(AHORA))
            .await
            .unwrap();
        let ResultadoVenta::Creada(v) = r else {
            panic!()
        };
        assert_eq!(v.fecha, dia(13));
        assert_eq!(v.fecha_original, Some(dia(12)));
    }

    /// Relación con Cierre: si hoy también está cerrado, la venta pasa a mañana.

    #[tokio::test]
    async fn si_hoy_tambien_esta_cerrado_pasa_a_manana() {
        let repo = Memoria {
            cierres: HashSet::from([dia(13)]),
            ..Default::default()
        };
        let r = registrar_venta(
            &repo,
            &catalogo(),
            &solicitud(6, "2026-10-13T20:10:00-06:00"),
            "cajero",
            ahora("2026-10-13T20:11:00-06:00"),
        )
        .await
        .unwrap();
        let ResultadoVenta::Creada(v) = r else {
            panic!()
        };
        assert_eq!(v.fecha, dia(14));
        assert_eq!(v.fecha_original, Some(dia(13)));
    }

    /// Una hora local más de 5 minutos en el futuro (reloj mal) se rechaza.

    #[tokio::test]
    async fn hora_en_el_futuro_es_422() {
        let repo = Memoria::default();
        let r = registrar_venta(
            &repo,
            &catalogo(),
            &solicitud(6, "2026-10-13T10:00:00-06:00"),
            "cajero",
            ahora(AHORA),
        )
        .await;
        assert!(matches!(r, Err(ErrorVenta::Validacion(_))));
    }
}
