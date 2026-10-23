///Define las estructuras de datos puras del reporte de vencimiento (modelos JSON) y las reglas de negocio para calcular los días restantes y el estado de cada lote.
use serde::{Deserialize, Serialize};

/// Resumen global con el conteo acumulado de unidades por estado de vencimiento.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumenVencimiento {
    pub vencido: i32,
    pub vence_hoy: i32,
    pub por_vencer: i32,
    pub vigente: i32,
}

/// Representación de un lote individual (hornada) y sus días de vida útil.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoteVencimiento {
    pub lote_id: String,
    pub producto_id: String,
    pub unidades: i32,
    pub fecha_vencimiento: String,
    pub dias_restantes: i64,
    pub estado: String, // Estados válidos: "vencido", "vence_hoy", "por_vencer", "vigente"
}

/// Documento/Respuesta principal del reporte de vencimientos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReporteVencimientos {
    pub fecha: String,
    pub actualizado_en: String,
    pub resumen: ResumenVencimiento,
    pub lotes: Vec<LoteVencimiento>,
}

impl ReporteVencimientos {
    /// Regla de negocio pura: Calcula los días de diferencia entre la fecha de vencimiento
    /// del lote y la fecha de la consulta, asignando el estado correspondiente según el contrato.
    pub fn calcular_estado_lote(fecha_vencimiento: &str, fecha_consulta: &str) -> (i64, String) {
        use chrono::NaiveDate;

        // Parseo de fechas en formato ISO (YYYY-MM-DD)
        let date_venc =
            NaiveDate::parse_from_str(fecha_vencimiento, "%Y-%m-%d").unwrap_or_default();
        let date_act = NaiveDate::parse_from_str(fecha_consulta, "%Y-%m-%d").unwrap_or_default();

        // Calculamos la diferencia en días
        let dias_restantes = (date_venc - date_act).num_days();

        // Evaluamos el estado según los días restantes
        let estado = match dias_restantes {
            d if d < 0 => "vencido".to_string(), // Ya pasó la fecha de vencimiento
            0 => "vence_hoy".to_string(),        // Vence el día de hoy
            1 => "por_vencer".to_string(),       // Le queda 1 día útil
            _ => "vigente".to_string(),          // Tiene más de 1 día de vida útil
        };

        (dias_restantes, estado)
    }
}
