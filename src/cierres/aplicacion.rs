use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
};

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::Deserialize;

use super::dominio::{Cierre, CierrePorProducto, DescarteCierre, ErrorCampo, SolicitudCierre};

// ─────────────────────────────────────────────────────────────────────────────
// Datos mínimos que necesita el cierre para leer documentos oficiales
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct HornadaDia {
    pub fecha: NaiveDate,
    pub producto_id: String,
    pub unidades: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LineaVentaDia {
    pub producto_id: String,
    pub cantidad: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VentaDia {
    pub fecha: NaiveDate,
    pub lineas: Vec<LineaVentaDia>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProductoCierre {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default = "producto_activo_por_defecto")]
    pub activo: bool,
}

fn producto_activo_por_defecto() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct CostoCierre {
    pub producto_id: String,
    pub costo_unitario_centavos: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigCierre {
    pub modo_produccion: String,
    pub costos: Vec<CostoCierre>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Contrato con la base de datos
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorRepositorio {
    Conflicto,
    NoDisponible(String),
}

pub trait RepositorioCierres {
    fn obtener_cierre(
        &self,
        fecha: NaiveDate,
    ) -> impl Future<Output = Result<Option<Cierre>, ErrorRepositorio>> + Send;

    fn listar_hornadas(
        &self,
        fecha: NaiveDate,
    ) -> impl Future<Output = Result<Vec<HornadaDia>, ErrorRepositorio>> + Send;

    fn listar_ventas(
        &self,
        fecha: NaiveDate,
    ) -> impl Future<Output = Result<Vec<VentaDia>, ErrorRepositorio>> + Send;

    fn listar_productos(
        &self,
    ) -> impl Future<Output = Result<Vec<ProductoCierre>, ErrorRepositorio>> + Send;

    fn obtener_config(
        &self,
    ) -> impl Future<Output = Result<Option<ConfigCierre>, ErrorRepositorio>> + Send;

    fn crear_cierre(
        &self,
        cierre: &Cierre,
    ) -> impl Future<Output = Result<(), ErrorRepositorio>> + Send;
}

// ─────────────────────────────────────────────────────────────────────────────
// Resultados del caso de uso
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum ResultadoCierre {
    Creado(Cierre),
    YaProcesado(Cierre),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorCierre {
    Validacion(Vec<ErrorCampo>),
    DiferenciaNegativa(Vec<ErrorCampo>),
    ConflictoContenido,
    NoDisponible(String),
}

impl From<ErrorRepositorio> for ErrorCierre {
    fn from(error: ErrorRepositorio) -> Self {
        match error {
            ErrorRepositorio::Conflicto => Self::ConflictoContenido,
            ErrorRepositorio::NoDisponible(motivo) => Self::NoDisponible(motivo),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Caso de uso: cerrar un día
// ─────────────────────────────────────────────────────────────────────────────

pub async fn cerrar_dia<R: RepositorioCierres>(
    repo: &R,
    solicitud: &SolicitudCierre,
    responsable: &str,
    ahora: DateTime<FixedOffset>,
) -> Result<ResultadoCierre, ErrorCierre> {
    solicitud.validar().map_err(ErrorCierre::Validacion)?;

    let huella = solicitud.huella();

    // 1. Idempotencia: si ya se cerró, no volver a calcular ni sobrescribir.
    if let Some(existente) = repo.obtener_cierre(solicitud.fecha).await? {
        return comparar_con_existente(existente, &huella);
    }

    // 2. Leer datos oficiales del día, el catálogo y la configuración vigente.
    let productos = repo.listar_productos().await?;
    let config = repo.obtener_config().await?.ok_or_else(|| {
        ErrorCierre::NoDisponible("No existe panaderia/config:vigente".to_string())
    })?;

    if !matches!(config.modo_produccion.as_str(), "modelo" | "lotes") {
        return Err(ErrorCierre::NoDisponible(
            "modo_produccion tiene un valor no reconocido".to_string(),
        ));
    }

    let mut productos_por_id = HashMap::new();

    for producto in productos {
        productos_por_id.insert(producto.id.clone(), producto);
    }

    let mut costos = HashMap::new();

    for costo in config.costos {
        if costo.costo_unitario_centavos < 0 {
            return Err(ErrorCierre::NoDisponible(format!(
                "Costo negativo para {}",
                costo.producto_id
            )));
        }

        costos.insert(costo.producto_id, costo.costo_unitario_centavos);
    }

    // 3. Sumar unidades producidas por producto.
    let hornadas = repo.listar_hornadas(solicitud.fecha).await?;
    let mut producidas: HashMap<String, i64> = HashMap::new();

    for hornada in hornadas {
        if hornada.fecha != solicitud.fecha {
            continue;
        }

        acumular(
            &mut producidas,
            hornada.producto_id,
            hornada.unidades,
            "produccion",
        )?;
    }

    // 4. Sumar unidades vendidas por producto.
    let ventas = repo.listar_ventas(solicitud.fecha).await?;
    let mut vendidas: HashMap<String, i64> = HashMap::new();

    for venta in ventas {
        if venta.fecha != solicitud.fecha {
            continue;
        }

        for linea in venta.lineas {
            acumular(&mut vendidas, linea.producto_id, linea.cantidad, "ventas")?;
        }
    }

    // 5. Convertir los descartes declarados en un mapa por producto.
    let descartadas: HashMap<String, i64> = solicitud
        .descartadas
        .iter()
        .map(|d: &DescarteCierre| (d.producto_id.clone(), d.unidades))
        .collect();

    for producto_id in descartadas.keys() {
        if !productos_por_id.contains_key(producto_id) {
            return Err(ErrorCierre::Validacion(vec![ErrorCampo::new(
                "descartadas.producto_id",
                &format!("El producto {producto_id} no existe en el catálogo"),
            )]));
        }
    }

    // Los documentos oficiales deben apuntar a productos existentes.
    for producto_id in producidas.keys().chain(vendidas.keys()) {
        if !productos_por_id.contains_key(producto_id) {
            return Err(ErrorCierre::NoDisponible(format!(
                "Los registros del día apuntan al producto inexistente {producto_id}"
            )));
        }
    }

    // 6. Incluir todos los productos activos, incluso si tienen valor cero.
    // También conservamos productos inactivos que sí tuvieron movimiento ese día.
    let mut ids: BTreeSet<String> = productos_por_id
        .values()
        .filter(|producto| producto.activo)
        .map(|producto| producto.id.clone())
        .collect();

    ids.extend(producidas.keys().cloned());
    ids.extend(vendidas.keys().cloned());
    ids.extend(
        descartadas
            .iter()
            .filter(|(_, unidades)| **unidades > 0)
            .map(|(id, _)| id.clone()),
    );

    // 7. Calcular los cierres por producto. No se guarda nada si hay errores.
    let mut por_producto = Vec::new();
    let mut errores = Vec::new();
    let mut diferencias_negativas = Vec::new();
    let mut total_merma_centavos = 0_i64;

    for producto_id in ids {
        let unidades_producidas = producidas.get(&producto_id).copied().unwrap_or(0);
        let unidades_vendidas = vendidas.get(&producto_id).copied().unwrap_or(0);
        let unidades_descartadas = descartadas.get(&producto_id).copied().unwrap_or(0);

        let costo = costos.get(&producto_id).copied().ok_or_else(|| {
            ErrorCierre::NoDisponible(format!(
                "Falta el costo vigente de {producto_id} en config:vigente"
            ))
        })?;

        match CierrePorProducto::calcular(
            &producto_id,
            unidades_producidas,
            unidades_vendidas,
            unidades_descartadas,
            costo,
        ) {
            Ok(producto) => {
                if producto.diferencia > 0 && !solicitud.confirmo_diferencia {
                    errores.push(ErrorCampo::new(
                        "confirmo_diferencia",
                        &format!("Debes confirmar la diferencia positiva de {producto_id}"),
                    ));
                }

                total_merma_centavos = total_merma_centavos
                    .checked_add(producto.merma_centavos)
                    .ok_or_else(|| {
                        ErrorCierre::NoDisponible(
                            "El total de merma supera el límite numérico".to_string(),
                        )
                    })?;

                por_producto.push(producto);
            }
            Err(error) => {
                let campo = format!("{}.{}", producto_id, error.campo);
                let detalle = ErrorCampo::new(&campo, &error.motivo);

                if error.campo == "diferencia"
                    && error.motivo == "Las unidades vendidas y descartadas superan las producidas"
                {
                    diferencias_negativas.push(detalle);
                } else {
                    errores.push(detalle);
                }
            }
        }
    }
    if !diferencias_negativas.is_empty() {
        return Err(ErrorCierre::DiferenciaNegativa(diferencias_negativas));
    }
    if !errores.is_empty() {
        return Err(ErrorCierre::Validacion(errores));
    }

    // 8. Construir el documento oficial. Existencia inicial y lotes finales
    // permanecen en cero/vacíos en esta versión del proyecto.
    let cierre = Cierre {
        id: format!("cierre:{}", solicitud.fecha),
        tipo: "cierre".to_string(),
        fecha: solicitud.fecha,
        responsable: responsable.to_string(),
        cerrado_en: ahora.to_rfc3339(),
        modo_produccion: config.modo_produccion,
        confirmo_diferencia: solicitud.confirmo_diferencia,
        huella,
        por_producto,
        total_merma_centavos,
    };

    // 9. Crear sin sobrescribir. CouchDB evita duplicados concurrentes.
    match repo.crear_cierre(&cierre).await {
        Ok(()) => Ok(ResultadoCierre::Creado(cierre)),
        Err(ErrorRepositorio::Conflicto) => {
            // Otro encargado pudo cerrar el día entre la lectura y la escritura.
            match repo.obtener_cierre(solicitud.fecha).await? {
                Some(existente) => comparar_con_existente(existente, &cierre.huella),
                None => Err(ErrorCierre::NoDisponible(
                    "CouchDB informó conflicto, pero el cierre no aparece".to_string(),
                )),
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn comparar_con_existente(existente: Cierre, huella: &str) -> Result<ResultadoCierre, ErrorCierre> {
    if existente.huella == huella {
        Ok(ResultadoCierre::YaProcesado(existente))
    } else {
        Err(ErrorCierre::ConflictoContenido)
    }
}

fn acumular(
    mapa: &mut HashMap<String, i64>,
    producto_id: String,
    cantidad: i64,
    origen: &str,
) -> Result<(), ErrorCierre> {
    if cantidad < 0 {
        return Err(ErrorCierre::NoDisponible(format!(
            "Se encontró una cantidad negativa en {origen}"
        )));
    }

    let total = mapa.entry(producto_id).or_insert(0);

    *total = total.checked_add(cantidad).ok_or_else(|| {
        ErrorCierre::NoDisponible(format!(
            "La suma de unidades de {origen} supera el límite numérico"
        ))
    })?;

    Ok(())
}
