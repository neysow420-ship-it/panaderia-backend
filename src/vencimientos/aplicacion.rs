//! Caso de uso del reporte de vencimientos: evalúa cada hornada y acumula el
//! resumen por estado.

use chrono::{FixedOffset, NaiveDate, Utc};

use super::couchdb::VencimientosCouchRepository;
use super::dominio::{LoteVencimiento, ReporteVencimientos, ResumenVencimiento};

/// Zona horaria del negocio: America/El_Salvador (UTC-6, sin horario de verano).
fn zona_negocio() -> FixedOffset {
    FixedOffset::west_opt(6 * 3600).expect("offset válido")
}

/// Caso de uso de la lógica de aplicación para la gestión de vencimientos.
pub struct VencimientoCasoUso {
    repo: VencimientosCouchRepository,
}

impl VencimientoCasoUso {
    /// Crea el caso de uso inyectando el repositorio de CouchDB.
    pub fn new(repo: VencimientosCouchRepository) -> Self {
        Self { repo }
    }

    /// Genera el reporte consolidado calculando los estados y sumando las
    /// unidades por categoría.
    pub async fn generar_reporte_vencimientos(
        &self,
        fecha_consulta: Option<String>,
    ) -> Result<ReporteVencimientos, String> {
        let now = Utc::now().with_timezone(&zona_negocio());
        let fecha_actual = match fecha_consulta {
            Some(texto) => NaiveDate::parse_from_str(&texto, "%Y-%m-%d")
                .map_err(|_| "fecha debe tener el formato AAAA-MM-DD".to_string())?
                .to_string(),
            None => now.format("%Y-%m-%d").to_string(),
        };
        let actualizado_en = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);

        // 1. Hornadas crudas desde CouchDB.
        let hornadas = self.repo.obtener_hornadas_activas().await?;

        let mut lotes = Vec::new();
        let mut resumen = ResumenVencimiento {
            vencido: 0,
            vence_hoy: 0,
            por_vencer: 0,
            vigente: 0,
        };

        // 2. Evaluar cada hornada y acumular unidades por estado.
        for doc in hornadas {
            let lote_id = doc["_id"].as_str().unwrap_or("").to_string();
            let producto_id = doc["producto_id"].as_str().unwrap_or("").to_string();
            let unidades = doc["unidades"].as_i64().unwrap_or(0) as i32;
            let fecha_vencimiento = doc["fecha_vencimiento"].as_str().unwrap_or("").to_string();

            // Sin fecha de vencimiento válida no hay nada que reportar.
            if fecha_vencimiento.is_empty() {
                continue;
            }

            let (dias_restantes, estado) =
                ReporteVencimientos::calcular_estado_lote(&fecha_vencimiento, &fecha_actual);

            match estado.as_str() {
                "vencido" => resumen.vencido += unidades,
                "vence_hoy" => resumen.vence_hoy += unidades,
                "por_vencer" => resumen.por_vencer += unidades,
                "vigente" => resumen.vigente += unidades,
                _ => {}
            }

            lotes.push(LoteVencimiento {
                lote_id,
                producto_id,
                unidades,
                fecha_vencimiento,
                dias_restantes,
                estado,
            });
        }

        Ok(ReporteVencimientos {
            fecha: fecha_actual,
            actualizado_en,
            resumen,
            lotes,
        })
    }
}
