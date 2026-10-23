#!/usr/bin/env bash
# Siembra los 5 productos y sus costos canónicos en CouchDB.
# Uso (desde la raíz del repo): ./scripts/sembrar-catalogo.sh
# Lee COUCHDB_URL, COUCHDB_USER y COUCHDB_PASSWORD del .env.
set -euo pipefail
cd "$(dirname "$0")/.."
set -a; [ -f .env ] && . ./.env; set +a
: "${COUCHDB_URL:?Falta COUCHDB_URL}" "${COUCHDB_USER:?Falta COUCHDB_USER}" "${COUCHDB_PASSWORD:?Falta COUCHDB_PASSWORD}"

python3 scripts/sembrar_catalogo.py
