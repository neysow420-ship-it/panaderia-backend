//! # Caso de uso: registrar una hornada
//!
//! Es el **paso a paso** de qué pasa cuando llega una hornada. Usa las reglas del
//! [`dominio`](super::dominio) y le pide los datos a un [`RepositorioHornadas`]
//! (CouchDB en producción, memoria en las pruebas).
//!
//! 1. **Validar** la solicitud (forma, ULID, producto, unidades, hora, vencimiento).
//! 2. **Idempotencia**: si `hornada:{ULID}` ya existe, comparar la huella:
//!    igual → `YaProcesada` (200); distinta → `ConflictoOperacion` (409).
//! 3. **Producto**: debe existir en `catalogo` (relación con **Producto y Config**);
//!    de ahí sale `vida_util_dias` para el vencimiento por defecto.
//! 4. **Armar y guardar**: `PUT` en CouchDB. Solo se responde éxito cuando CouchDB
//!    confirmó la escritura.
//!
//! La idempotencia se revisa **antes** de buscar el producto: un reintento legítimo
//! sigue dando 200 aunque el catálogo haya cambiado entre un envío y otro.

use std::future::Future;

use chrono::{DateTime, Duration, FixedOffset};

use super::dominio::{
    ErrorCampo, Hornada, ProductoCatalogo, SolicitudHornada, TOLERANCIA_FUTURO_SEG,
    construir_hornada,
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
pub trait RepositorioHornadas {
    /// Busca una hornada por su `_id`. `Ok(None)` si no existe.
    fn obtener_hornada(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<Hornada>, ErrorRepositorio>> + Send;
    /// Crea la hornada. Si el `_id` ya existe devuelve `Err(Conflicto)`.
    fn crear_hornada(
        &self,
        hornada: &Hornada,
    ) -> impl Future<Output = Result<(), ErrorRepositorio>> + Send;
    /// Relación con **Producto y Config**: lee `catalogo/{producto_id}`.
    /// `Ok(None)` si el producto no existe.
    fn obtener_producto(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<ProductoCatalogo>, ErrorRepositorio>> + Send;
}

// ─────────────────────────────────────────────────────────────────────────────
// Resultados
// ─────────────────────────────────────────────────────────────────────────────

/// Resultado cuando todo sale bien. La API lo traduce a un código HTTP.
#[derive(Debug, PartialEq, Eq)]
pub enum ResultadoHornada {
    /// La hornada era nueva y se guardó → **201**.
    Creada(Hornada),
    /// La hornada ya estaba guardada con el mismo contenido (reenvío) → **200**.
    YaProcesada(Hornada),
}

/// Resultado cuando algo falla. La API lo traduce a un código HTTP.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorHornada {
    /// Datos inválidos, con la lista de errores por campo → **422**.
    Validacion(Vec<ErrorCampo>),
    /// Ya existe una hornada con ese ULID pero con otro contenido → **409**.
    ConflictoOperacion,
    /// CouchDB no responde → **503** (el cliente reintenta más tarde).
    NoDisponible(String),
}

/// Permite usar `?` con errores del repositorio dentro del caso de uso:
/// convierte automáticamente un `ErrorRepositorio` en `ErrorHornada`.
impl From<ErrorRepositorio> for ErrorHornada {
    fn from(e: ErrorRepositorio) -> Self {
        match e {
            ErrorRepositorio::Conflicto => ErrorHornada::ConflictoOperacion,
            ErrorRepositorio::NoDisponible(m) => ErrorHornada::NoDisponible(m),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Caso de uso
// ─────────────────────────────────────────────────────────────────────────────

/// Registra una hornada siguiendo los 4 pasos del inicio de este archivo.
///
/// - `repo`: acceso a la base (CouchDB o memoria).
/// - `solicitud`: lo que mandó el cliente.
/// - `usuario`: quién registra (relación con **Login**, viene del middleware).
/// - `ahora`: hora del servidor. Se pasa como parámetro para poder probar con
///   cualquier fecha.
pub async fn registrar_hornada<R: RepositorioHornadas>(
    repo: &R,
    solicitud: &SolicitudHornada,
    usuario: &str,
    ahora: DateTime<FixedOffset>,
) -> Result<ResultadoHornada, ErrorHornada> {
    // ── Paso 1: validar la forma de la solicitud ──
    let validada = solicitud.validar().map_err(ErrorHornada::Validacion)?;
    // Una hornada "del futuro" significa que el reloj del dispositivo está mal y
    // movería la producción a otro día.
    if validada.creado > ahora + Duration::seconds(TOLERANCIA_FUTURO_SEG) {
        return Err(ErrorHornada::Validacion(vec![ErrorCampo::new(
            "creado_local",
            "la hora está en el futuro; revise el reloj del dispositivo",
        )]));
    }

    // ── Paso 2: idempotencia, ¿ya llegó esta hornada antes? ──
    let huella = solicitud.huella();
    if let Some(existente) = repo.obtener_hornada(&solicitud.id_documento()).await? {
        return comparar_con_existente(existente, &huella);
    }

    // ── Paso 3: el producto debe existir (relación con Producto y Config) ──
    let Some(producto) = repo.obtener_producto(&solicitud.producto_id).await? else {
        return Err(ErrorHornada::Validacion(vec![ErrorCampo::new(
            "producto_id",
            "producto no existe en el catálogo",
        )]));
    };

    // ── Paso 4: armar el documento oficial y guardarlo ──
    let hornada = construir_hornada(solicitud, &validada, &producto, usuario)
        .map_err(ErrorHornada::Validacion)?;

    match repo.crear_hornada(&hornada).await {
        // CouchDB confirmó: ahora sí se responde éxito.
        Ok(()) => Ok(ResultadoHornada::Creada(hornada)),
        // Dos envíos de la misma hornada llegaron casi al mismo tiempo y el otro
        // la guardó primero. Se lee la guardada y se compara la huella.
        Err(ErrorRepositorio::Conflicto) => match repo.obtener_hornada(&hornada.id).await? {
            Some(existente) => comparar_con_existente(existente, &huella),
            None => Err(ErrorHornada::NoDisponible(
                "conflicto sin documento".to_string(),
            )),
        },
        // Cualquier otro error de la base.
        Err(e) => Err(e.into()),
    }
}

/// Compara una hornada ya guardada con la huella de la que acaba de llegar.
/// Misma huella → reenvío (200). Distinta → conflicto (409).
fn comparar_con_existente(
    existente: Hornada,
    huella: &str,
) -> Result<ResultadoHornada, ErrorHornada> {
    if existente.huella == huella {
        Ok(ResultadoHornada::YaProcesada(existente))
    } else {
        Err(ErrorHornada::ConflictoOperacion)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas del caso de uso, con una base en memoria en lugar de CouchDB
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod pruebas {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use chrono::NaiveDate;

    use super::*;

    /// Repositorio en memoria que imita a CouchDB (implementa el mismo trait).
    #[derive(Default)]
    struct Memoria {
        hornadas: Mutex<HashMap<String, Hornada>>,
        productos: HashMap<String, ProductoCatalogo>,
        /// Simula que otro envío guardó el documento justo antes que nosotros.
        carrera: Mutex<Option<Hornada>>,
    }

    impl Memoria {
        /// Base con el catálogo mínimo de las pruebas: pan francés (1 día) y semita (2 días).
        fn con_catalogo() -> Self {
            let productos = [("producto:pan-frances", 1), ("producto:semita", 2)]
                .into_iter()
                .map(|(id, vida)| {
                    (
                        id.to_string(),
                        ProductoCatalogo {
                            id: id.to_string(),
                            vida_util_dias: vida,
                        },
                    )
                })
                .collect();
            Self {
                productos,
                ..Default::default()
            }
        }
    }

    impl RepositorioHornadas for Memoria {
        async fn obtener_hornada(&self, id: &str) -> Result<Option<Hornada>, ErrorRepositorio> {
            Ok(self.hornadas.lock().unwrap().get(id).cloned())
        }
        async fn crear_hornada(&self, hornada: &Hornada) -> Result<(), ErrorRepositorio> {
            if let Some(otra) = self.carrera.lock().unwrap().take() {
                self.hornadas.lock().unwrap().insert(otra.id.clone(), otra);
            }
            let mut h = self.hornadas.lock().unwrap();
            if h.contains_key(&hornada.id) {
                return Err(ErrorRepositorio::Conflicto);
            }
            h.insert(hornada.id.clone(), hornada.clone());
            Ok(())
        }
        async fn obtener_producto(
            &self,
            id: &str,
        ) -> Result<Option<ProductoCatalogo>, ErrorRepositorio> {
            Ok(self.productos.get(id).cloned())
        }
    }

    const CREADO: &str = "2026-10-13T05:30:00-06:00";
    const AHORA: &str = "2026-10-13T05:30:10-06:00";

    fn solicitud(unidades: i64) -> SolicitudHornada {
        SolicitudHornada {
            operacion_id: "01J9ZK5A2B3C4D5E6F7G8H9J0K".into(),
            producto_id: "producto:pan-frances".into(),
            unidades,
            creado_local: CREADO.into(),
            fecha_vencimiento: None,
        }
    }

    fn ahora(texto: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(texto).unwrap()
    }

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    /// Hornada nueva: se guarda y se responde Creada (201).
    #[tokio::test]
    async fn primera_vez_crea_la_hornada() {
        let repo = Memoria::con_catalogo();
        let r = registrar_hornada(&repo, &solicitud(150), "encargado", ahora(AHORA))
            .await
            .unwrap();
        let ResultadoHornada::Creada(h) = r else {
            panic!("debía crearse")
        };
        assert_eq!(h.id, "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K");
        assert_eq!(h.unidades, 150);
        assert_eq!(h.usuario, "encargado");
        assert_eq!(h.fecha, dia(13));
        // Sin fecha_vencimiento en la solicitud: fecha + vida útil (1 día).
        assert_eq!(h.fecha_vencimiento, dia(14));
        assert_eq!(repo.hornadas.lock().unwrap().len(), 1);
    }

    /// El vencimiento por defecto depende de la vida útil de cada producto.
    #[tokio::test]
    async fn vencimiento_por_defecto_sigue_la_vida_util_del_producto() {
        let repo = Memoria::con_catalogo();
        let mut s = solicitud(80);
        s.producto_id = "producto:semita".into();
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        let ResultadoHornada::Creada(h) = r else {
            panic!("debía crearse")
        };
        assert_eq!(h.fecha_vencimiento, dia(15));
    }

    /// Si el cliente manda `fecha_vencimiento`, esa es la que se guarda.
    #[tokio::test]
    async fn respeta_el_vencimiento_que_manda_el_cliente() {
        let repo = Memoria::con_catalogo();
        let mut s = solicitud(150);
        s.fecha_vencimiento = Some("2026-10-16".into());
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        let ResultadoHornada::Creada(h) = r else {
            panic!("debía crearse")
        };
        assert_eq!(h.fecha_vencimiento, dia(16));
    }

    /// Mismo ULID y mismo contenido: no se duplica y se responde YaProcesada (200).
    #[tokio::test]
    async fn reenvio_igual_devuelve_ya_procesada() {
        let repo = Memoria::con_catalogo();
        let s = solicitud(150);
        registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        assert!(matches!(r, ResultadoHornada::YaProcesada(_)));
        assert_eq!(repo.hornadas.lock().unwrap().len(), 1);
    }

    /// Un reintento sigue dando 200 aunque el producto ya no esté en el catálogo.
    #[tokio::test]
    async fn reenvio_no_depende_de_que_el_producto_siga_existiendo() {
        let mut repo = Memoria::con_catalogo();
        let s = solicitud(150);
        registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        repo.productos.clear();
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        assert!(matches!(r, ResultadoHornada::YaProcesada(_)));
    }

    /// Mismo ULID con otras unidades: ConflictoOperacion (409).
    #[tokio::test]
    async fn mismo_id_con_otro_contenido_es_409() {
        let repo = Memoria::con_catalogo();
        registrar_hornada(&repo, &solicitud(150), "encargado", ahora(AHORA))
            .await
            .unwrap();
        let r = registrar_hornada(&repo, &solicitud(151), "encargado", ahora(AHORA)).await;
        assert_eq!(r, Err(ErrorHornada::ConflictoOperacion));
        assert_eq!(repo.hornadas.lock().unwrap().len(), 1);
    }

    /// Dos envíos iguales al mismo tiempo: el segundo recibe 409 de CouchDB, relee y responde 200.
    #[tokio::test]
    async fn carrera_entre_dos_envios_iguales_no_falla() {
        let repo = Memoria::con_catalogo();
        let s = solicitud(150);
        // Preparamos la "otra" copia guardada justo en medio.
        let previa =
            match registrar_hornada(&Memoria::con_catalogo(), &s, "encargado", ahora(AHORA))
                .await
                .unwrap()
            {
                ResultadoHornada::Creada(h) => h,
                _ => unreachable!(),
            };
        *repo.carrera.lock().unwrap() = Some(previa);
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA))
            .await
            .unwrap();
        assert!(matches!(r, ResultadoHornada::YaProcesada(_)));
    }

    /// Dos envíos con el mismo ULID pero distinto contenido en carrera: 409.
    #[tokio::test]
    async fn carrera_con_contenido_distinto_es_409() {
        let repo = Memoria::con_catalogo();
        let previa = match registrar_hornada(
            &Memoria::con_catalogo(),
            &solicitud(150),
            "encargado",
            ahora(AHORA),
        )
        .await
        .unwrap()
        {
            ResultadoHornada::Creada(h) => h,
            _ => unreachable!(),
        };
        *repo.carrera.lock().unwrap() = Some(previa);
        let r = registrar_hornada(&repo, &solicitud(999), "encargado", ahora(AHORA)).await;
        assert_eq!(r, Err(ErrorHornada::ConflictoOperacion));
    }

    /// Un producto que no está en el catálogo da 422 sobre `producto_id` y no guarda nada.
    #[tokio::test]
    async fn producto_inexistente_es_422() {
        let repo = Memoria::con_catalogo();
        let mut s = solicitud(150);
        s.producto_id = "producto:no-existe".into();
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA)).await;
        let Err(ErrorHornada::Validacion(errores)) = r else {
            panic!("debía ser validación")
        };
        assert_eq!(errores[0].campo, "producto_id");
        assert!(repo.hornadas.lock().unwrap().is_empty());
    }

    /// Datos inválidos (unidades 0) dan 422 sin tocar la base.
    #[tokio::test]
    async fn unidades_cero_es_422() {
        let repo = Memoria::con_catalogo();
        let r = registrar_hornada(&repo, &solicitud(0), "encargado", ahora(AHORA)).await;
        assert!(matches!(r, Err(ErrorHornada::Validacion(_))));
        assert!(repo.hornadas.lock().unwrap().is_empty());
    }

    /// Una hora local más de 5 minutos en el futuro (reloj mal) se rechaza.
    #[tokio::test]
    async fn hora_en_el_futuro_es_422() {
        let repo = Memoria::con_catalogo();
        let mut s = solicitud(150);
        s.creado_local = "2026-10-13T07:00:00-06:00".into();
        let r = registrar_hornada(&repo, &s, "encargado", ahora(AHORA)).await;
        let Err(ErrorHornada::Validacion(errores)) = r else {
            panic!("debía ser validación")
        };
        assert_eq!(errores[0].campo, "creado_local");
    }

    /// Una hornada hecha de madrugada en UTC cuenta para el día de El Salvador.
    #[tokio::test]
    async fn el_dia_de_negocio_usa_la_hora_de_el_salvador() {
        let repo = Memoria::con_catalogo();
        let mut s = solicitud(150);
        // 01:30 UTC del 14 son las 19:30 del 13 en El Salvador.
        s.creado_local = "2026-10-14T01:30:00Z".into();
        let r = registrar_hornada(&repo, &s, "encargado", ahora("2026-10-14T01:30:05Z"))
            .await
            .unwrap();
        let ResultadoHornada::Creada(h) = r else {
            panic!("debía crearse")
        };
        assert_eq!(h.fecha, dia(13));
        assert_eq!(h.fecha_vencimiento, dia(14));
    }
}
