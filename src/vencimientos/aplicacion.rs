///Caso de uso. Procesa las hornadas traídas desde CouchDB, invoca la lógica de dominio para evaluar cada lote y genera el resumen acumulado.


use super::dominio::{LoteVencimiento, ReporteVencimientos, ResumenVencimiento};
use super::couchdb::VencimientosCouchRepository;
use chrono::Local;

/// Caso de uso de la lógica de aplicación para la gestión de vencimientos.
pub struct VencimientoCasoUso {
    repo: VencimientosCouchRepository,
}

impl VencimientoCasoUso {
    /// Crea una nueva instancia del caso de uso inyectando el repositorio de CouchDB.
    pub fn new(repo: VencimientosCouchRepository) -> Self {
        Self { repo }
    }

    /// Genera el reporte consolidado calculando los estados y sumando las unidades por categoría.
    pub async fn generar_reporte_vencimientos(&self, fecha_consulta: Option<String>) -> Result<ReporteVencimientos, String> {
        let now = Local::now();
        // Si no se provee una fecha específica de consulta, se toma la fecha actual local
        let fecha_actual = fecha_consulta.unwrap_or_else(|| now.format("%Y-%m-%d").to_string());
        let actualizado_en = now.to_rfc3339();

        // 1. Obtenemos las hornadas crudas desde CouchDB
        let hornadas = self.repo.obtener_hornadas_activas().await?;

        let mut lotes = Vec::new();
        let mut resumen = ResumenVencimiento {
            vencido: 0,
            vence_hoy: 0,
            por_vencer: 0,
            vigente: 0,
        };

        // 2. Iteramos cada documento de hornada traído de la base de datos
        for doc in hornadas {
            let lote_id = doc["_id"].as_str().unwrap_or("").to_string();
            let producto_id = doc["producto_id"].as_str().unwrap_or("").to_string();
            let unidades = doc["unidades"].as_i64().unwrap_or(0) as i32;
            let fecha_vencimiento = doc["fecha_vencimiento"].as_str().unwrap_or("").to_string();

            // Si el documento no tiene fecha de vencimiento válida, lo ignoramos
            if fecha_vencimiento.is_empty() {
                continue;
            }

            // 3. Calculamos días restantes y estado llamando a la función de dominio
            let (dias_restantes, estado) = ReporteVencimientos::calcular_estado_lote(&fecha_vencimiento, &fecha_actual);

            // 4. Sumamos las unidades al resumen según el estado determinado
            match estado.as_str() {
                "vencido" => resumen.vencido += unidades,
                "vence_hoy" => resumen.vence_hoy += unidades,
                "por_vencer" => resumen.por_vencer += unidades,
                "vigente" => resumen.vigente += unidades,
                _ => {}
            }

            // 5. Agregamos el lote procesado al listado final
            lotes.push(LoteVencimiento {
                lote_id,
                producto_id,
                unidades,
                fecha_vencimiento,
                dias_restantes,
                estado,
            });
        }

        // 6. Retornamos el objeto estructurado listo para la API
        Ok(ReporteVencimientos {
            fecha: fecha_actual,
            actualizado_en,
            resumen,
            lotes,
        })
    }
}