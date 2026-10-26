use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    future::Future,
};

use chrono::NaiveDate;
use serde::Deserialize;

use super::dominio::{
    DiaMerma, DiaVentas, ErrorCampo, MermaPorProducto, ParametrosRango, ReporteMerma,
    ReporteVentas, VentaPorProducto,
};
/// Línea de venta con los importes oficiales guardados en CouchDB.
#[derive(Clone, Debug)]
pub struct LineaVentaReporte {
    pub producto_id: String,
    pub cantidad: i64,
    pub subtotal_centavos: i64,
}

/// Venta oficial con su fecha de negocio.
#[derive(Clone, Debug)]
pub struct VentaReporte {
    pub fecha: NaiveDate,
    pub lineas: Vec<LineaVentaReporte>,
}

/// Error al leer los documentos oficiales.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorRepositorio {
    NoDisponible(String),
}
/// Cierre oficial leído de panaderia/cierre:{fecha}.
#[derive(Clone, Debug, Deserialize)]
pub struct CierreGuardado {
    pub por_producto: Vec<MermaPorProducto>,
    pub total_merma_centavos: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HornadaMerma {
    pub fecha: NaiveDate,
    pub producto_id: String,
    pub unidades: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DescarteMerma {
    pub fecha: String,
    pub producto_id: String,
    pub unidades: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProductoMerma {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default = "activo_por_defecto")]
    pub activo: bool,
}

fn activo_por_defecto() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct CostoMerma {
    pub producto_id: String,
    pub costo_unitario_centavos: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigMerma {
    pub costos: Vec<CostoMerma>,
}

/// Operaciones que necesita el reporte.
pub trait RepositorioReportes {
    fn listar_ventas(
        &self,
        desde: NaiveDate,
        hasta: NaiveDate,
    ) -> impl Future<Output = Result<Vec<VentaReporte>, ErrorRepositorio>> + Send;
    fn obtener_cierre(
        &self,
        fecha: NaiveDate,
    ) -> impl Future<Output = Result<Option<CierreGuardado>, ErrorRepositorio>> + Send;

    fn listar_hornadas(
        &self,
        desde: NaiveDate,
        hasta: NaiveDate,
    ) -> impl Future<Output = Result<Vec<HornadaMerma>, ErrorRepositorio>> + Send;

    fn listar_descartes(
        &self,
        desde: NaiveDate,
        hasta: NaiveDate,
    ) -> impl Future<Output = Result<Vec<DescarteMerma>, ErrorRepositorio>> + Send;

    fn listar_productos(
        &self,
    ) -> impl Future<Output = Result<Vec<ProductoMerma>, ErrorRepositorio>> + Send;

    fn obtener_config(
        &self,
    ) -> impl Future<Output = Result<Option<ConfigMerma>, ErrorRepositorio>> + Send;
}

/// Errores que devuelve el caso de uso.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorReporte {
    Validacion(Vec<ErrorCampo>),
    NoDisponible(String),
}

impl From<ErrorRepositorio> for ErrorReporte {
    fn from(error: ErrorRepositorio) -> Self {
        match error {
            ErrorRepositorio::NoDisponible(motivo) => Self::NoDisponible(motivo),
        }
    }
}

/// Genera el reporte agrupado por día y producto.
///
/// Usa la fecha de negocio que quedó guardada en cada venta,
/// no la fecha en que el backend recibió la solicitud.
pub async fn reporte_ventas<R: RepositorioReportes>(
    repo: &R,
    rango: &ParametrosRango,
) -> Result<ReporteVentas, ErrorReporte> {
    rango.validar().map_err(ErrorReporte::Validacion)?;

    let ventas = repo.listar_ventas(rango.desde, rango.hasta).await?;

    // BTreeMap mantiene los días y productos en orden.
    let mut agrupadas: BTreeMap<NaiveDate, BTreeMap<String, VentaPorProducto>> = BTreeMap::new();

    // Incluir todos los días del rango, aunque no tengan ventas.
    for fecha in rango.fechas() {
        agrupadas.insert(fecha, BTreeMap::new());
    }

    for venta in ventas {
        let Some(productos) = agrupadas.get_mut(&venta.fecha) else {
            return Err(ErrorReporte::NoDisponible(
                "El repositorio devolvió una venta fuera del rango solicitado".to_string(),
            ));
        };

        for linea in venta.lineas {
            if linea.cantidad <= 0 || linea.subtotal_centavos < 0 {
                return Err(ErrorReporte::NoDisponible(
                    "Se encontró una línea de venta con cantidades o importes inválidos"
                        .to_string(),
                ));
            }

            let producto = productos
                .entry(linea.producto_id.clone())
                .or_insert_with(|| VentaPorProducto {
                    producto_id: linea.producto_id,
                    unidades: 0,
                    total_centavos: 0,
                });

            producto.unidades = producto
                .unidades
                .checked_add(linea.cantidad)
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible(
                        "La suma de unidades supera el límite numérico".to_string(),
                    )
                })?;

            producto.total_centavos = producto
                .total_centavos
                .checked_add(linea.subtotal_centavos)
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible(
                        "La suma de importes supera el límite numérico".to_string(),
                    )
                })?;
        }
    }

    let mut dias = Vec::new();
    let mut total_centavos = 0_i64;

    for (fecha, productos) in agrupadas {
        let por_producto: Vec<VentaPorProducto> = productos.into_values().collect();

        for producto in &por_producto {
            total_centavos = total_centavos
                .checked_add(producto.total_centavos)
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible(
                        "El total del reporte supera el límite numérico".to_string(),
                    )
                })?;
        }

        dias.push(DiaVentas {
            fecha,
            por_producto,
        });
    }

    Ok(ReporteVentas {
        desde: rango.desde,
        hasta: rango.hasta,
        dias,
        total_centavos,
    })
}
#[derive(Default)]
struct TotalesMerma {
    producidas: i64,
    vendidas: i64,
    descartadas: i64,
}

/// Genera el reporte leyendo cierres históricos y calculando
/// una vista previa para hoy si el día todavía no está cerrado.
pub async fn reporte_merma<R: RepositorioReportes>(
    repo: &R,
    rango: &ParametrosRango,
    hoy: NaiveDate,
    actualizado_en: String,
) -> Result<ReporteMerma, ErrorReporte> {
    rango.validar().map_err(ErrorReporte::Validacion)?;

    let mut dias = Vec::new();
    let mut total_reporte = 0_i64;

    for fecha in rango.fechas() {
        // Los días cerrados se leen del documento inmutable.
        if let Some(cierre) = repo.obtener_cierre(fecha).await? {
            total_reporte = total_reporte
                .checked_add(cierre.total_merma_centavos)
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible(
                        "El total del reporte supera el límite numérico".into(),
                    )
                })?;

            dias.push(DiaMerma {
                fecha,
                cerrado: true,
                por_producto: cierre.por_producto,
                total_merma_centavos: cierre.total_merma_centavos,
            });

            continue;
        }

        // No inventamos cierres para días pasados o futuros sin documento.
        // Solo hoy se presenta una vista previa.
        if fecha != hoy {
            continue;
        }

        let hornadas = repo.listar_hornadas(hoy, hoy).await?;
        let ventas = repo.listar_ventas(hoy, hoy).await?;
        let descartes = repo.listar_descartes(hoy, hoy).await?;
        let productos = repo.listar_productos().await?;

        let config = repo.obtener_config().await?.ok_or_else(|| {
            ErrorReporte::NoDisponible("No existe panaderia/config:vigente".into())
        })?;

        let mut totales: HashMap<String, TotalesMerma> = HashMap::new();

        for hornada in hornadas {
            if hornada.fecha != hoy || hornada.unidades < 0 {
                return Err(ErrorReporte::NoDisponible(
                    "Hornada con fecha o unidades inválidas".into(),
                ));
            }

            let total = totales.entry(hornada.producto_id).or_default();
            total.producidas = total
                .producidas
                .checked_add(hornada.unidades)
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible(
                        "La suma de producción supera el límite numérico".into(),
                    )
                })?;
        }

        for venta in ventas {
            if venta.fecha != hoy {
                continue;
            }

            for linea in venta.lineas {
                if linea.cantidad < 0 || linea.subtotal_centavos < 0 {
                    return Err(ErrorReporte::NoDisponible(
                        "Línea de venta con cantidades o importes inválidos".into(),
                    ));
                }

                let total = totales.entry(linea.producto_id).or_default();
                total.vendidas = total.vendidas.checked_add(linea.cantidad).ok_or_else(|| {
                    ErrorReporte::NoDisponible("La suma de ventas supera el límite numérico".into())
                })?;
            }
        }

        for descarte in descartes {
            let fecha_descarte = descarte.fecha.parse::<NaiveDate>().map_err(|_| {
                ErrorReporte::NoDisponible("Existe un descarte con fecha inválida".into())
            })?;

            if fecha_descarte != hoy || descarte.unidades < 0 {
                return Err(ErrorReporte::NoDisponible(
                    "Descarte con fecha o unidades inválidas".into(),
                ));
            }

            let total = totales.entry(descarte.producto_id).or_default();
            total.descartadas = total
                .descartadas
                .checked_add(descarte.unidades)
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible(
                        "La suma de descartes supera el límite numérico".into(),
                    )
                })?;
        }

        // Incluimos productos activos incluso sin movimientos.
        let mut ids: BTreeSet<String> = productos
            .iter()
            .filter(|producto| producto.activo)
            .map(|producto| producto.id.clone())
            .collect();

        // También conservamos productos con movimiento aunque estén inactivos.
        ids.extend(totales.keys().cloned());

        let costos: HashMap<String, i64> = config
            .costos
            .into_iter()
            .map(|costo| (costo.producto_id, costo.costo_unitario_centavos))
            .collect();

        let mut por_producto = Vec::new();
        let mut total_dia = 0_i64;

        for producto_id in ids {
            let cantidades = totales.remove(&producto_id).unwrap_or_default();

            let costo = costos.get(&producto_id).copied().ok_or_else(|| {
                ErrorReporte::NoDisponible(format!("Falta el costo vigente de {producto_id}"))
            })?;

            if costo < 0 {
                return Err(ErrorReporte::NoDisponible(format!(
                    "El costo de {producto_id} es negativo"
                )));
            }

            let diferencia = cantidades
                .producidas
                .checked_sub(cantidades.vendidas)
                .and_then(|n| n.checked_sub(cantidades.descartadas))
                .ok_or_else(|| {
                    ErrorReporte::NoDisponible("No se pudo calcular la diferencia".into())
                })?;

            // La vista previa muestra la diferencia incluso si es negativa.
            // Es el cierre oficial el que la rechazará al guardarse.
            let merma = cantidades.descartadas.checked_mul(costo).ok_or_else(|| {
                ErrorReporte::NoDisponible("La merma supera el límite numérico".into())
            })?;

            total_dia = total_dia.checked_add(merma).ok_or_else(|| {
                ErrorReporte::NoDisponible("El total diario supera el límite numérico".into())
            })?;

            por_producto.push(MermaPorProducto {
                producto_id,
                producidas: cantidades.producidas,
                vendidas: cantidades.vendidas,
                descartadas: cantidades.descartadas,
                diferencia,
                merma_centavos: merma,
                costo_aplicado_centavos: costo,
            });
        }

        total_reporte = total_reporte.checked_add(total_dia).ok_or_else(|| {
            ErrorReporte::NoDisponible("El total del reporte supera el límite numérico".into())
        })?;

        dias.push(DiaMerma {
            fecha: hoy,
            cerrado: false,
            por_producto,
            total_merma_centavos: total_dia,
        });
    }

    Ok(ReporteMerma {
        desde: rango.desde,
        hasta: rango.hasta,
        actualizado_en,
        dias,
        total_merma_centavos: total_reporte,
    })
}
