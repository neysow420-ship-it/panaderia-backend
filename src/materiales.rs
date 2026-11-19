//! Las facturas aportan unidades del SKU facturado. Las hornadas guardan
//! el consumo en millonesimas de esa misma unidad.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const ESCALA: i64 = 1_000_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ingrediente {
    pub sku: String,
    pub millonesimas: i64,
}

pub fn receta_valida(receta: &[Ingrediente]) -> bool {
    let mut skus = BTreeSet::new();
    (1..=50).contains(&receta.len())
        && receta.iter().all(|item| {
            !item.sku.is_empty()
                && item.sku.len() <= 100
                && item.sku.trim() == item.sku
                && !item.sku.chars().any(char::is_control)
                && (1..=1_000_000_000).contains(&item.millonesimas)
                && skus.insert(&item.sku)
        })
}

#[derive(Default, Serialize)]
pub struct Existencia {
    pub descripcion: String,
    pub saldo_millonesimas: i64,
}

#[derive(Deserialize)]
pub struct FacturaGuardada {
    pub factura: Entradas,
}

#[derive(Deserialize)]
pub struct Entradas {
    pub lineas: Vec<Entrada>,
}

#[derive(Deserialize)]
pub struct Entrada {
    sku: String,
    descripcion: String,
    cantidad: u32,
}

pub fn calcular(
    facturas: Vec<FacturaGuardada>,
    consumos: impl IntoIterator<Item = Ingrediente>,
) -> Option<BTreeMap<String, Existencia>> {
    let mut inventario = BTreeMap::<String, Existencia>::new();
    for linea in facturas.into_iter().flat_map(|doc| doc.factura.lineas) {
        let material = inventario.entry(linea.sku).or_default();
        material.descripcion = linea.descripcion;
        material.saldo_millonesimas = material
            .saldo_millonesimas
            .checked_add(i64::from(linea.cantidad) * ESCALA)?;
    }
    for item in consumos {
        let material = inventario.entry(item.sku).or_default();
        material.saldo_millonesimas = material.saldo_millonesimas.checked_sub(item.millonesimas)?;
    }
    Some(inventario)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn llegada_de_factura_compensa_deuda() {
        let consumo = Ingrediente {
            sku: "harina".into(),
            millonesimas: 1_500_000,
        };
        assert_eq!(
            calcular(vec![], [consumo.clone()]).unwrap()["harina"].saldo_millonesimas,
            -1_500_000
        );
        let factura = serde_json::from_value(serde_json::json!({
            "factura": {"lineas": [{"sku": "harina", "descripcion": "Harina", "cantidad": 2}]}
        }))
        .unwrap();
        assert_eq!(
            calcular(vec![factura], [consumo]).unwrap()["harina"].saldo_millonesimas,
            500_000
        );
    }
}
