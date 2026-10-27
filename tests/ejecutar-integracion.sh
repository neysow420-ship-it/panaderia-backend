#!/usr/bin/env bash
# Ejecuta las pruebas de integración contra el CouchDB del .env.
#
# 1. Comprueba que CouchDB responde.
# 2. Aplica la configuración y la siembra (salvo PANADERIA_SALTAR_SETUP=1).
# 3. Ejecuta las pruebas de integración en serie.
#
# Uso (desde la raíz del repo):
#   ./tests/ejecutar-integracion.sh
#
# Requisitos: CouchDB levantado con `docker compose ... up -d couchdb`.
set -euo pipefail
cd "$(dirname "$0")/.."
ENV_FILE="${PANADERIA_ENV_FILE:-.env}"
set -a; [ -f "$ENV_FILE" ] && . "$ENV_FILE"; set +a

: "${COUCHDB_URL:?Falta COUCHDB_URL en .env}" \
  "${COUCHDB_PASSWORD:?Falta COUCHDB_PASSWORD en .env}" \
  "${BACKEND_PASSWORD:?Falta BACKEND_PASSWORD en .env}"

CODIGO=$(curl -sS -m 5 -o /dev/null -w '%{http_code}' "$COUCHDB_URL/" || true)
if [ "$CODIGO" = "000" ]; then
  echo "CouchDB no responde en $COUCHDB_URL" >&2
  echo "Levántalo con: docker compose --env-file .env -f infra/docker-compose.yml up -d couchdb" >&2
  exit 1
fi

if [ "${PANADERIA_SALTAR_SETUP:-0}" != "1" ]; then
  ./infra/couchdb/setup.sh
fi

export PANADERIA_INTEGRACION=1
export BACKEND_USER="${BACKEND_USER:-backend}"
export AUTH_MODE=activo

cargo test --test integracion -- --test-threads=1
