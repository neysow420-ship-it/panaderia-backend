#!/usr/bin/env bash
# Comprueba los permisos de la fase 3.8:
#   - cliente lee catalogo;
#   - cliente NO escribe catalogo;
#   - cliente NO lee panaderia ni usuarios;
#   - backend escribe catalogo (y la validación rechaza datos inválidos).
#
# Uso: ./infra/couchdb/prueba-permisos.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
ENV_FILE="${PANADERIA_ENV_FILE:-.env}"
set -a; [ -f "$ENV_FILE" ] && . "$ENV_FILE"; set +a

: "${COUCHDB_URL:?Falta COUCHDB_URL}" \
  "${BACKEND_PASSWORD:?Falta BACKEND_PASSWORD}" \
  "${CLIENTE_PASSWORD:?Falta CLIENTE_PASSWORD}"

BASE="${COUCHDB_URL%/}"
CLIENTE="cliente:$CLIENTE_PASSWORD"
BACKEND="backend:$BACKEND_PASSWORD"
FALLOS=0

estado() { # estado <usuario:clave> <método> <ruta> [datos]
  local credenciales="$1" metodo="$2" ruta="$3" datos="${4:-}"
  if [ -n "$datos" ]; then
    curl -sS -o /dev/null -u "$credenciales" -H 'Content-Type: application/json' \
      -X "$metodo" --data-binary "$datos" -w '%{http_code}' "$BASE/$ruta"
  else
    curl -sS -o /dev/null -u "$credenciales" -X "$metodo" -w '%{http_code}' "$BASE/$ruta"
  fi
}

comprobar() { # comprobar <descripción> <código obtenido> <códigos esperados...>
  local descripcion="$1" obtenido="$2"; shift 2
  local esperado
  for esperado in "$@"; do
    if [ "$obtenido" = "$esperado" ]; then
      echo "  OK   $descripcion ($obtenido)"
      return
    fi
  done
  echo "  FALLA $descripcion: se obtuvo $obtenido, se esperaba $*" >&2
  FALLOS=$((FALLOS + 1))
}

echo "Probando permisos de CouchDB"

comprobar "cliente lee catalogo" \
  "$(estado "$CLIENTE" GET catalogo/_all_docs)" 200

comprobar "cliente no escribe catalogo" \
  "$(estado "$CLIENTE" PUT catalogo/producto:prueba-cliente \
    '{"tipo":"producto","nombre":"Prueba","precio_centavos":1,"vida_util_dias":1,"activo":true}')" \
  401 403

comprobar "cliente no lee panaderia" \
  "$(estado "$CLIENTE" GET panaderia/_all_docs)" 401 403

comprobar "cliente no lee usuarios" \
  "$(estado "$CLIENTE" GET usuarios/_all_docs)" 401 403

ID="producto:prueba-permisos-$$"
comprobar "backend escribe catalogo" \
  "$(estado "$BACKEND" PUT "catalogo/$ID" \
    '{"tipo":"producto","nombre":"Prueba permisos","precio_centavos":1,"vida_util_dias":1,"activo":true}')" \
  201 202

comprobar "validación rechaza precio negativo" \
  "$(estado "$BACKEND" PUT "catalogo/${ID}-malo" \
    '{"tipo":"producto","nombre":"Mal","precio_centavos":-1,"vida_util_dias":1,"activo":true}')" \
  401 403

REV=$(curl -sS -u "$BACKEND" "$BASE/catalogo/$ID" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["_rev"])')
comprobar "backend borra el documento de prueba" \
  "$(estado "$BACKEND" DELETE "catalogo/$ID?rev=$REV")" 200 202

if [ "$FALLOS" -ne 0 ]; then
  echo "Fallaron $FALLOS comprobaciones." >&2
  exit 1
fi
echo "Permisos correctos."
