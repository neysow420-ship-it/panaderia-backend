use std::collections::HashSet;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Error de validación que la API devolverá al cliente.
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

/// Producto y unidades descartadas que declara el encargado.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DescarteCierre {
    pub producto_id: String,
    pub unidades: i64,
}

/// Datos que recibe POST /cierre.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SolicitudCierre {
    pub fecha: NaiveDate,
    pub descartadas: Vec<DescarteCierre>,
    pub confirmo_diferencia: bool,
}

/// Resumen de un producto dentro del cierre.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CierrePorProducto {
    pub producto_id: String,
    pub existencia_inicial: i64,
    pub producidas: i64,
    pub vendidas: i64,
    pub descartadas: i64,
    pub diferencia: i64,
    pub existencia_final: i64,
    pub lotes_finales: Vec<Value>,
    pub costo_aplicado_centavos: i64,
    pub merma_centavos: i64,
    pub merma_bp: i64,
}

/// Documento oficial e inmutable cierre:{fecha}.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Cierre {
    #[serde(rename = "_id")]
    pub id: String,
    pub tipo: String,
    pub fecha: NaiveDate,
    pub responsable: String,
    pub cerrado_en: String,
    pub modo_produccion: String,
    pub confirmo_diferencia: bool,
    pub huella: String,
    pub por_producto: Vec<CierrePorProducto>,
    pub total_merma_centavos: i64,
}

impl SolicitudCierre {
    /// Valida las cantidades declaradas antes de consultar la base.
    pub fn validar(&self) -> Result<(), Vec<ErrorCampo>> {
        let mut errores = Vec::new();
        let mut productos = HashSet::new();

        for (i, descarte) in self.descartadas.iter().enumerate() {
            if descarte.producto_id.trim().is_empty() {
                errores.push(ErrorCampo::new(
                    &format!("descartadas[{i}].producto_id"),
                    "El producto es obligatorio",
                ));
            }

            if !(0..=100_000).contains(&descarte.unidades) {
                errores.push(ErrorCampo::new(
                    &format!("descartadas[{i}].unidades"),
                    "Debe estar entre 0 y 100000",
                ));
            }

            if !productos.insert(&descarte.producto_id) {
                errores.push(ErrorCampo::new(
                    &format!("descartadas[{i}].producto_id"),
                    "No repita un producto; agrupe sus unidades en una sola entrada",
                ));
            }
        }

        if errores.is_empty() {
            Ok(())
        } else {
            Err(errores)
        }
    }

    /// Calcula una huella SHA-256 estable para reconocer reenvíos idénticos.
    ///
    /// Ordenar las entradas hace que cambiar el orden de los productos
    /// no convierta una misma solicitud en un cierre diferente.
    pub fn huella(&self) -> String {
        let mut canonica = self.clone();

        canonica
            .descartadas
            .sort_by(|a, b| a.producto_id.cmp(&b.producto_id));

        let contenido = serde_json::to_vec(&canonica).expect("SolicitudCierre se puede serializar");

        let hash = Sha256::digest(contenido);

        format!(
            "sha256:{}",
            hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }
}

impl CierrePorProducto {
    /// Calcula el resumen y rechaza una diferencia negativa.
    pub fn calcular(
        producto_id: &str,
        producidas: i64,
        vendidas: i64,
        descartadas: i64,
        costo_aplicado_centavos: i64,
    ) -> Result<Self, ErrorCampo> {
        if producidas < 0 || vendidas < 0 || descartadas < 0 {
            return Err(ErrorCampo::new(
                "unidades",
                "Las cantidades no pueden ser negativas",
            ));
        }

        if costo_aplicado_centavos < 0 {
            return Err(ErrorCampo::new(
                "costo_aplicado_centavos",
                "El costo no puede ser negativo",
            ));
        }

        let diferencia = producidas
            .checked_sub(vendidas)
            .and_then(|n| n.checked_sub(descartadas))
            .ok_or_else(|| ErrorCampo::new("diferencia", "No se pudo calcular la diferencia"))?;

        if diferencia < 0 {
            return Err(ErrorCampo::new(
                "diferencia",
                "Las unidades vendidas y descartadas superan las producidas",
            ));
        }

        let merma_centavos = descartadas
            .checked_mul(costo_aplicado_centavos)
            .ok_or_else(|| {
                ErrorCampo::new("merma_centavos", "El importe supera el límite permitido")
            })?;

        let merma_bp = if producidas == 0 {
            0
        } else {
            ((i128::from(descartadas) * 10_000) / i128::from(producidas)) as i64
        };

        Ok(Self {
            producto_id: producto_id.to_string(),
            existencia_inicial: 0,
            producidas,
            vendidas,
            descartadas,
            diferencia,
            existencia_final: diferencia,
            lotes_finales: Vec::new(),
            costo_aplicado_centavos,
            merma_centavos,
            merma_bp,
        })
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn calcula_merma_y_diferencia() {
        let producto = CierrePorProducto::calcular("producto:pan-frances", 100, 70, 10, 5).unwrap();

        assert_eq!(producto.diferencia, 20);
        assert_eq!(producto.existencia_final, 20);
        assert_eq!(producto.merma_centavos, 50);
        assert_eq!(producto.merma_bp, 1000);
    }

    #[test]
    fn rechaza_diferencia_negativa() {
        let resultado = CierrePorProducto::calcular("producto:pan-frances", 10, 8, 3, 5);

        assert!(resultado.is_err());
    }

    #[test]
    fn la_huella_no_depende_del_orden_de_las_entradas() {
        let a = SolicitudCierre {
            fecha: NaiveDate::from_ymd_opt(2026, 10, 26).unwrap(),
            descartadas: vec![
                DescarteCierre {
                    producto_id: "producto:semita".into(),
                    unidades: 2,
                },
                DescarteCierre {
                    producto_id: "producto:pan-frances".into(),
                    unidades: 3,
                },
            ],
            confirmo_diferencia: true,
        };

        let mut b = a.clone();
        b.descartadas.reverse();

        assert_eq!(a.huella(), b.huella());
    }
}
