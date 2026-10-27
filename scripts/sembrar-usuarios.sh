#!/usr/bin/env bash
# Siembra los dos usuarios de aplicación con hash Argon2id.
#
# Las contraseñas se leen de .env (o del entorno) y nunca se escriben en Git.
# Uso (desde la raíz del repo): ./scripts/sembrar-usuarios.sh
#
# Si `cargo` no está en el host pero Docker sí, ejecuta el mismo binario dentro
# del contenedor de la API. Así funciona tanto en desarrollo como en el servidor.
set -euo pipefail
cd "$(dirname "$0")/.."
ENV_FILE="${PANADERIA_ENV_FILE:-.env}"
set -a; [ -f "$ENV_FILE" ] && . "$ENV_FILE"; set +a

: "${COUCHDB_URL:?Falta COUCHDB_URL}" \
  "${BACKEND_PASSWORD:?Falta BACKEND_PASSWORD}" \
  "${CAJERO_PASSWORD:?Falta CAJERO_PASSWORD}" \
  "${ENCARGADO_PASSWORD:?Falta ENCARGADO_PASSWORD}"

export BACKEND_USER="${BACKEND_USER:-backend}"
export CAJERO_USUARIO="${CAJERO_USUARIO:-cajero1}"
export ENCARGADO_USUARIO="${ENCARGADO_USUARIO:-encargado1}"

if command -v cargo >/dev/null 2>&1; then
  cargo run --quiet --bin sembrar_usuarios
elif command -v docker >/dev/null 2>&1; then
  docker compose --env-file "$ENV_FILE" -f infra/docker-compose.yml run --rm \
    -e BACKEND_USER -e BACKEND_PASSWORD \
    -e CAJERO_USUARIO -e CAJERO_PASSWORD \
    -e ENCARGADO_USUARIO -e ENCARGADO_PASSWORD \
    api cargo run --quiet --bin sembrar_usuarios
else
  echo "Se necesita cargo (Rust) o Docker para sembrar los usuarios." >&2
  exit 1
fi
