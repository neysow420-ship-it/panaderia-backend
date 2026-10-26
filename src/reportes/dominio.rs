use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Parámetros que reciben GET /reportes/ventas y GET /merma.
#[derive(Clone, Debug, Deserialize)]
pub struct ParametrosRango {
    pub desde: NaiveDate,
    pub hasta: NaiveDate,
}

/// Error de validación del rango solicitado.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ErrorCampo {
    pub campo: String,
    pub motivo: String,
}

impl ErrorCampo {
    pub fn new(campo: &str, motivo: &str) -> Self {
        Self {
            campo: campo.to_string(),
            motivo: motivo.to_string(),
        }
    }
}

impl ParametrosRango {
    /// Valida el orden y la longitud máxima del rango.
    /// Se permiten hasta 31 fechas contando ambos extremos.
    pub fn validar(&self) -> Result<(), Vec<ErrorCampo>> {
        let mut errores = Vec::new();

        if self.hasta < self.desde {
            errores.push(ErrorCampo::new(
                "hasta",
                "Debe ser igual o posterior a desde",
            ));
        } else if self.hasta.signed_duration_since(self.desde).num_days() > 30 {
            errores.push(ErrorCampo::new(
                "hasta",
                "El rango no puede superar 31 días, contando ambos extremos",
            ));
        }

        if errores.is_empty() {
            Ok(())
        } else {
            Err(errores)
        }
    }

    /// Devuelve todas las fechas del rango, incluidos ambos extremos.
    pub fn fechas(&self) -> Vec<NaiveDate> {
        let dias = self.hasta.signed_duration_since(self.desde).num_days();

        if !(0..=30).contains(&dias) {
            return Vec::new();
        }

        (0..=dias).map(|n| self.desde + Duration::days(n)).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reporte de ventas
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct VentaPorProducto {
    pub producto_id: String,
    pub unidades: i64,
    pub total_centavos: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DiaVentas {
    pub fecha: NaiveDate,
    pub por_producto: Vec<VentaPorProducto>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReporteVentas {
    pub desde: NaiveDate,
    pub hasta: NaiveDate,
    pub dias: Vec<DiaVentas>,
    pub total_centavos: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Reporte de merma
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MermaPorProducto {
    pub producto_id: String,
    pub producidas: i64,
    pub vendidas: i64,
    pub descartadas: i64,
    pub diferencia: i64,
    pub merma_centavos: i64,
    pub costo_aplicado_centavos: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DiaMerma {
    pub fecha: NaiveDate,
    pub cerrado: bool,
    pub por_producto: Vec<MermaPorProducto>,
    pub total_merma_centavos: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReporteMerma {
    pub desde: NaiveDate,
    pub hasta: NaiveDate,
    pub actualizado_en: String,
    pub dias: Vec<DiaMerma>,
    pub total_merma_centavos: i64,
}
