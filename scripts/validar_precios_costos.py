#!/usr/bin/env python3
"""Valida los precios públicos y los costos internos de la panadería."""

from __future__ import annotations

import json
import re
import sys
from datetime import date
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
DATOS = RAIZ / "scripts" / "datos"
PRODUCTOS_ESPERADOS = 5


def validar_fecha(valor: object, contexto: str) -> None:
    if not isinstance(valor, str):
        raise ValueError(f"{contexto}: la fecha debe ser texto AAAA-MM-DD.")

    try:
        if date.fromisoformat(valor).isoformat() != valor:
            raise ValueError
    except ValueError:
        raise ValueError(f"{contexto}: fecha inválida: {valor!r}.") from None


def validar_entero(
    documento: dict, campo: str, contexto: str, minimo: int
) -> None:
    valor = documento.get(campo)

    # bool también es considerado int por Python; por eso se excluye.
    if isinstance(valor, bool) or not isinstance(valor, int):
        raise ValueError(f"{contexto}: {campo} debe ser un entero.")

    if valor < minimo:
        raise ValueError(
            f"{contexto}: {campo} debe ser como mínimo {minimo}."
        )


def cargar_y_validar_datos() -> tuple[dict, dict]:
    try:
        catalogo = json.loads(
            (DATOS / "catalogo.json").read_text(encoding="utf-8")
        )
        config = json.loads(
            (DATOS / "config.json").read_text(encoding="utf-8")
        )
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"No se pudieron leer los JSON: {error}") from error

    if not isinstance(catalogo, dict) or not isinstance(config, dict):
        raise ValueError("El catálogo y la configuración deben ser objetos JSON.")

    validar_fecha(catalogo.get("vigente_desde"), "Catálogo")
    validar_fecha(config.get("vigente_desde"), "Configuración")

    if catalogo["vigente_desde"] != config["vigente_desde"]:
        raise ValueError(
            "El catálogo y la configuración deben compartir "
            "la misma fecha vigente."
        )

    productos = catalogo.get("productos")

    if not isinstance(productos, list):
        raise ValueError("catalogo.json debe contener una lista de productos.")

    if len(productos) != PRODUCTOS_ESPERADOS:
        raise ValueError(
            f"Se esperaban {PRODUCTOS_ESPERADOS} productos, "
            f"pero hay {len(productos)}."
        )

    slugs = set()

    for producto in productos:
        if not isinstance(producto, dict):
            raise ValueError("Cada producto debe ser un objeto JSON.")

        slug = producto.get("slug")
        contexto = f"Producto {slug!r}"

        if not isinstance(slug, str) or not re.fullmatch(
            r"[a-z0-9-]{1,64}", slug
        ):
            raise ValueError(
                f"{contexto}: slug inválido; utiliza minúsculas, "
                "números y guiones."
            )

        if slug in slugs:
            raise ValueError(f"Slug duplicado: {slug}.")

        slugs.add(slug)

        if not isinstance(producto.get("nombre"), str) or not producto[
            "nombre"
        ].strip():
            raise ValueError(f"{contexto}: falta un nombre válido.")

        validar_entero(producto, "precio_centavos", contexto, 1)
        validar_entero(producto, "vida_util_dias", contexto, 1)

        if any("costo" in campo.lower() for campo in producto):
            raise ValueError(
                f"{contexto}: no incluyas costos internos en el catálogo público."
            )

    costos = config.get("costos")

    if not isinstance(costos, list):
        raise ValueError("config.json debe contener una lista de costos.")

    productos_esperados = {f"producto:{slug}" for slug in slugs}
    productos_con_costo = set()

    for costo in costos:
        if not isinstance(costo, dict):
            raise ValueError("Cada costo debe ser un objeto JSON.")

        producto_id = costo.get("producto_id")

        if not isinstance(producto_id, str):
            raise ValueError("Cada costo debe tener un producto_id válido.")

        if producto_id not in productos_esperados:
            raise ValueError(
                f"Costo asociado a un producto inexistente: {producto_id}."
            )

        if producto_id in productos_con_costo:
            raise ValueError(f"Costo duplicado para {producto_id}.")

        validar_entero(
            costo,
            "costo_unitario_centavos",
            f"Costo de {producto_id}",
            0,
        )

        productos_con_costo.add(producto_id)

    faltantes = productos_esperados - productos_con_costo

    if faltantes:
        raise ValueError(
            "Faltan costos para: " + ", ".join(sorted(faltantes))
        )

    return catalogo, config


def main() -> None:
    catalogo, _ = cargar_y_validar_datos()

    print("Validación correcta.")
    print(f"Productos: {len(catalogo['productos'])}")
    print("Precios: enteros positivos en centavos.")
    print("Costos: enteros no negativos en centavos.")
    print("Separación entre catálogo y costos: correcta.")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        raise SystemExit(1)