#!/usr/bin/env bash
# Crea las bases y sube la vista de productos a CouchDB.
# Uso (desde la raíz del repo):  ./scripts/couchdb-productos.sh [--prueba]
#   --prueba  además crea 2 productos de ejemplo en `catalogo` y el documento `config:vigente`
#             en `panaderia`, para probar /productos y /config.
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

# La vista vive en `catalogo`. Si el design doc ya existe, se reemplaza usando su _rev.
REV=$(curl -s -u "$AUTH" "$COUCHDB_URL/catalogo/_design/productos" | sed -n 's/.*"_rev": *"\([^"]*\)".*/\1/p')
DOC=infra/couchdb/productos/_design_productos.json
echo "Vista productos_activos:"
if [ -n "$REV" ]; then
  sed "s/\"_id\": \"_design\/productos\",/\"_id\": \"_design\/productos\", \"_rev\": \"$REV\",/" "$DOC" | C -X PUT "$COUCHDB_URL/catalogo/_design/productos" -d @-
else
  C -X PUT "$COUCHDB_URL/catalogo/_design/productos" -d @"$DOC"
fi

if [ "${1:-}" = "--prueba" ]; then
  echo "Productos de prueba (409 = ya existía, está bien):"
  C -X PUT "$COUCHDB_URL/catalogo/producto:pan-frances" -d '{"tipo":"producto","nombre":"Pan francés","precio_centavos":10,"vida_util_dias":1,"activo":true}'
  C -X PUT "$COUCHDB_URL/catalogo/producto:donas" -d '{"tipo":"producto","nombre":"Donas","precio_centavos":35,"vida_util_dias":2,"activo":true}'
  echo "Config de prueba:"
  C -X PUT "$COUCHDB_URL/panaderia/config:vigente" -d '{"tipo":"config","vigente_desde":"2026-10-13","modo_produccion":"modelo","alerta_dias":1,"costos":[{"producto_id":"producto:pan-frances","costo_unitario_centavos":5},{"producto_id":"producto:donas","costo_unitario_centavos":18}]}'
fi