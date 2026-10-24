#!/usr/bin/env python3
"""Siembra el catálogo y los costos canónicos en CouchDB.

- Precios y vida útil → base `catalogo` (documentos `producto:{slug}`).
- Costos → base `panaderia` (documento `config:vigente`, campo `costos`).

Es idempotente: si un documento ya existe, conserva su `_rev` y los campos que
no administra este script. Lee la URL y las credenciales de las variables de
entorno (las carga `sembrar-catalogo.sh` desde `.env`).
"""

from __future__ import annotations

import base64
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from validar_precios_costos import cargar_y_validar_datos

RAIZ = Path(__file__).resolve().parent.parent
DATOS = RAIZ / "scripts" / "datos"


def pedir(metodo: str, url: str, cuerpo: dict | None = None) -> tuple[int, dict]:
    datos = None
    cabeceras = {"Content-Type": "application/json"}
    if cuerpo is not None:
        datos = json.dumps(cuerpo, ensure_ascii=False).encode("utf-8")

    credenciales = f"{os.environ['COUCHDB_USER']}:{os.environ['COUCHDB_PASSWORD']}"
    cabeceras["Authorization"] = "Basic " + base64.b64encode(
        credenciales.encode("utf-8")
    ).decode("ascii")

    peticion = urllib.request.Request(url, data=datos, headers=cabeceras, method=metodo)
    try:
        with urllib.request.urlopen(peticion, timeout=10) as respuesta:
            texto = respuesta.read().decode("utf-8")
            return respuesta.status, json.loads(texto) if texto else {}
    except urllib.error.HTTPError as error:
        texto = error.read().decode("utf-8")
        return error.code, json.loads(texto) if texto else {}


def guardar(base: str, doc: dict) -> None:
    url = f"{base}/{doc['_id']}"
    estado, existente = pedir("GET", url)
    if estado == 200:
        # Conserva _rev y los campos que agreguen otras partes.
        nuevo = {**existente, **doc}
        nuevo["_rev"] = existente["_rev"]
    elif estado == 404:
        nuevo = doc
    else:
        raise SystemExit(f"No se pudo leer {doc['_id']} ({estado}): {existente}")

    estado, respuesta = pedir("PUT", url, nuevo)
    if estado not in (201, 202):
        raise SystemExit(f"No se pudo guardar {doc['_id']} ({estado}): {respuesta}")
    print(f"  {doc['_id']}: {estado} ({respuesta.get('rev', 'sin rev')})")


def main() -> None:
    base = os.environ["COUCHDB_URL"].rstrip("/")
    #modificado
    catalogo, config = cargar_y_validar_datos()
    print(f"Sembrando catálogo en {base}/catalogo")
    for producto in catalogo["productos"]:
        slug = producto["slug"]
        doc = {
            "_id": f"producto:{slug}",
            "tipo": "producto",
            "nombre": producto["nombre"],
            "precio_centavos": producto["precio_centavos"],
            "vida_util_dias": producto["vida_util_dias"],
            "activo": True,
        }
        guardar(f"{base}/catalogo", doc)

    print(f"Sembrando configuración en {base}/panaderia")
    doc_config = {
        "_id": "config:vigente",
        "tipo": "config",
        "vigente_desde": config["vigente_desde"],
        "modo_produccion": config["modo_produccion"],
        "alerta_dias": config["alerta_dias"],
        "costos": config["costos"],
    }
    guardar(f"{base}/panaderia", doc_config)

    print(f"Listo: {len(catalogo['productos'])} productos y sus costos.")


if __name__ == "__main__":
    try:
        main()
    except KeyError as falta:
        print(f"Falta la variable de entorno {falta}", file=sys.stderr)
        raise SystemExit(1)
