#!/usr/bin/env bash
# Crea la vista y los índices Mango de hornadas en CouchDB.
# Uso (desde la raíz del repo): ./scripts/couchdb-hornadas.sh
# Lee COUCHDB_URL, COUCHDB_USER y COUCHDB_PASSWORD del .env.
set -euo pipefail
cd "$(dirname "$0")/.."
ENV_FILE="${PANADERIA_ENV_FILE:-.env}"
set -a; [ -f "$ENV_FILE" ] && . "$ENV_FILE"; set +a
: "${COUCHDB_URL:?Falta COUCHDB_URL}" "${COUCHDB_USER:?Falta COUCHDB_USER}" "${COUCHDB_PASSWORD:?Falta COUCHDB_PASSWORD}"
AUTH="$COUCHDB_USER:$COUCHDB_PASSWORD"
C() { curl -s -u "$AUTH" -H 'Content-Type: application/json' "$@"; echo; }

echo "Base panaderia:"
C -X PUT "$COUCHDB_URL/panaderia"   # 412 = ya existía, está bien

# Si el design doc ya existe, se reemplaza usando su _rev.
REV=$(curl -s -u "$AUTH" "$COUCHDB_URL/panaderia/_design/hornadas" | sed -n 's/.*"_rev": *"\([^"]*\)".*/\1/p')
DOC=infra/couchdb/hornadas/_design_hornadas.json
echo "Vista hornadas_por_dia_producto:"
if [ -n "$REV" ]; then
  sed "s/\"_id\": \"_design\/hornadas\",/\"_id\": \"_design\/hornadas\", \"_rev\": \"$REV\",/" "$DOC" | C -X PUT "$COUCHDB_URL/panaderia/_design/hornadas" -d @-
else
  C -X PUT "$COUCHDB_URL/panaderia/_design/hornadas" -d @"$DOC"
fi

echo "Índices Mango de hornadas:"
C -X POST "$COUCHDB_URL/panaderia/_index" -d @infra/couchdb/hornadas/indice_hornadas_fecha.json
C -X POST "$COUCHDB_URL/panaderia/_index" -d @infra/couchdb/hornadas/indice_hornadas_producto.json
