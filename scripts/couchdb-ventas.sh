#!/usr/bin/env bash
# Crea las bases y sube la vista e índice de ventas a CouchDB.
# Uso (desde la raíz del repo):  ./scripts/couchdb-ventas.sh [--prueba]
#   --prueba  además crea 2 productos de ejemplo en `catalogo` para probar POST /ventas.
# Lee COUCHDB_URL, COUCHDB_USER y COUCHDB_PASSWORD del .env.
set -euo pipefail
cd "$(dirname "$0")/.."
set -a; [ -f .env ] && . ./.env; set +a
: "${COUCHDB_URL:?Falta COUCHDB_URL}" "${COUCHDB_USER:?Falta COUCHDB_USER}" "${COUCHDB_PASSWORD:?Falta COUCHDB_PASSWORD}"
AUTH="$COUCHDB_USER:$COUCHDB_PASSWORD"
C() { curl -s -u "$AUTH" -H 'Content-Type: application/json' "$@"; echo; }

for bd in panaderia catalogo; do
  echo "Base $bd:"; C -X PUT "$COUCHDB_URL/$bd"   # 412 = ya existía, está bien
done

# Si el design doc ya existe, se reemplaza usando su _rev.
REV=$(curl -s -u "$AUTH" "$COUCHDB_URL/panaderia/_design/ventas" | sed -n 's/.*"_rev": *"\([^"]*\)".*/\1/p')
DOC=infra/couchdb/ventas/_design_ventas.json
echo "Vista ventas_por_dia_producto:"
if [ -n "$REV" ]; then
  sed "s/\"_id\": \"_design\/ventas\",/\"_id\": \"_design\/ventas\", \"_rev\": \"$REV\",/" "$DOC" | C -X PUT "$COUCHDB_URL/panaderia/_design/ventas" -d @-
else
  C -X PUT "$COUCHDB_URL/panaderia/_design/ventas" -d @"$DOC"
fi

echo "Índice Mango tipo+fecha:"
C -X POST "$COUCHDB_URL/panaderia/_index" -d @infra/couchdb/ventas/indice_ventas_fecha.json

if [ "${1:-}" = "--prueba" ]; then
  echo "Productos de prueba:"
  C -X PUT "$COUCHDB_URL/catalogo/producto:pan-frances" -d '{"tipo":"producto","nombre":"Pan francés","precio_centavos":10,"vida_util_dias":1,"activo":true}'
  C -X PUT "$COUCHDB_URL/catalogo/producto:donas" -d '{"tipo":"producto","nombre":"Donas","precio_centavos":35,"vida_util_dias":2,"activo":true}'
fi
