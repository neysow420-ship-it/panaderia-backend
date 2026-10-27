"""Pruebas de los datos canónicos de siembra.

Se ejecutan con:

    python3 -m unittest discover -s scripts/pruebas

Fijan el catálogo de 5 productos del contrato para que un cambio accidental
no altere los totales, la merma ni la comparación con la línea base.
"""

import sys
import unittest
from pathlib import Path

RAIZ = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(RAIZ / "scripts"))

from validar_precios_costos import cargar_y_validar_datos  # noqa: E402

PRECIOS_ESPERADOS = {
    "producto:pan-frances": 10,
    "producto:semita": 25,
    "producto:hojitas": 15,
    "producto:cachos": 30,
    "producto:donas": 35,
}

COSTOS_ESPERADOS = {
    "producto:pan-frances": 5,
    "producto:semita": 12,
    "producto:hojitas": 8,
    "producto:cachos": 15,
    "producto:donas": 18,
}


class DatosCanonicos(unittest.TestCase):
    def setUp(self) -> None:
        self.catalogo, self.config = cargar_y_validar_datos()

    def test_catalogo_tiene_exactamente_los_cinco_productos_del_contrato(self) -> None:
        precios = {
            f"producto:{producto['slug']}": producto["precio_centavos"]
            for producto in self.catalogo["productos"]
        }

        self.assertEqual(precios, PRECIOS_ESPERADOS)

    def test_costos_coinciden_con_el_contrato(self) -> None:
        costos = {
            costo["producto_id"]: costo["costo_unitario_centavos"]
            for costo in self.config["costos"]
        }

        self.assertEqual(costos, COSTOS_ESPERADOS)

    def test_catalogo_y_config_comparten_vigencia(self) -> None:
        self.assertEqual(
            self.catalogo["vigente_desde"],
            self.config["vigente_desde"],
        )


if __name__ == "__main__":
    unittest.main()
