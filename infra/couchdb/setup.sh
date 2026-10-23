#!/usr/bin/env bash
# Configura CouchDB para el sistema de la panadería:
#   - crea las bases panaderia, catalogo y usuarios;
#   - crea los usuarios backend (administrador) y cliente (solo lectura de catalogo);
#   - aplica _security por base;
#   - instala validate_doc_update en catalogo;
#   - siembra los 5 productos y sus costos.
#
# Uso (desde la raíz del repo): ./infra/couchdb/setup.sh
# Lee COUCHDB_URL, COUCHDB_USER, COUCHDB_PASSWORD, BACKEND_PASSWORD y
# CLIENTE_PASSWORD del .env.
set -euo pipefail
cd "$(dirname "$0")/../.."
set -a; [ -f .env ] && . ./.env; set +a

: "${COUCHDB_URL:?Falta COUCHDB_URL}" \
  "${COUCHDB_USER:?Falta COUCHDB_USER}" \
  "${COUCHDB_PASSWORD:?Falta COUCHDB_PASSWORD}" \
  "${BACKEND_PASSWORD:?Falta BACKEND_PASSWORD en .env}" \
  "${CLIENTE_PASSWORD:?Falta CLIENTE_PASSWORD en .env}"

BASE="${COUCHDB_URL%/}"
AUTH="$COUCHDB_USER:$COUCHDB_PASSWORD"

# Ejecuta una petición y separa cuerpo y código HTTP.
http() {
  local metodo="$1" ruta="$2" datos="${3:-}"
  if [ -n "$datos" ]; then
    curl -sS -u "$AUTH" -H 'Content-Type: application/json' -X "$metodo" \
      --data-binary "$datos" -w $'\n%{http_code}' "$BASE/$ruta"
  else
    curl -sS -u "$AUTH" -X "$metodo" -w $'\n%{http_code}' "$BASE/$ruta"
  fi
}

cuerpo() { printf '%s' "${1%$'\n'*}"; }
codigo() { printf '%s' "${1##*$'\n'}"; }

# Crea una base si no existe (412 = ya existía).
crear_base() {
  local bd="$1" respuesta estado
  respuesta=$(http PUT "$bd")
  estado=$(codigo "$respuesta")
  case "$estado" in
    201) echo "  base $bd: creada" ;;
    412) echo "  base $bd: ya existía" ;;
    *) echo "  base $bd: error $estado: $(cuerpo "$respuesta")" >&2; exit 1 ;;
  esac
}

# Crea o actualiza un usuario de CouchDB conservando su _rev.
crear_usuario() {
  local nombre="$1" contrasena="$2" rol="$3" ruta rev="" respuesta estado datos
  ruta="_users/org.couchdb.user:$nombre"
  respuesta=$(http GET "$ruta")
  if [ "$(codigo "$respuesta")" = "200" ]; then
    rev=$(cuerpo "$respuesta" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("_rev",""))')
  fi
  datos=$(python3 - "$nombre" "$contrasena" "$rol" "$rev" <<'PY'
import json, sys
nombre, contrasena, rol, rev = sys.argv[1:5]
doc = {
    "_id": f"org.couchdb.user:{nombre}",
    "name": nombre,
    "type": "user",
    "roles": [rol],
    "password": contrasena,
}
if rev:
    doc["_rev"] = rev
print(json.dumps(doc))
PY
)
  respuesta=$(http PUT "$ruta" "$datos")
  estado=$(codigo "$respuesta")
  if [ "$estado" = "201" ] || [ "$estado" = "202" ]; then
    echo "  usuario $nombre: listo (rol $rol)"
  else
    echo "  usuario $nombre: error $estado: $(cuerpo "$respuesta")" >&2
    exit 1
  fi
}

# Aplica la lista de administradores y miembros de una base.
seguridad() {
  local bd="$1" admins="$2" members="$3" respuesta estado
  respuesta=$(http PUT "$bd/_security" "{\"admins\":{\"roles\":$admins},\"members\":{\"roles\":$members}}")
  estado=$(codigo "$respuesta")
  if [ "$estado" = "200" ]; then
    echo "  seguridad $bd: aplicada"
  else
    echo "  seguridad $bd: error $estado: $(cuerpo "$respuesta")" >&2
    exit 1
  fi
}

echo "1. Bases"
crear_base panaderia
crear_base catalogo
crear_base usuarios
crear_base _users

echo "2. Usuarios de aplicación"
crear_usuario backend "$BACKEND_PASSWORD" backend
crear_usuario cliente "$CLIENTE_PASSWORD" cliente

echo "3. Permisos por base"
# catalogo: backend escribe (validate_doc_update lo exige); cliente solo lee.
seguridad catalogo '["backend"]' '["backend","cliente"]'
# panaderia y usuarios: solo backend.
seguridad panaderia '["backend"]' '["backend"]'
seguridad usuarios '["backend"]' '["backend"]'

echo "4. validate_doc_update de catalogo"
RESPUESTA_DDOC=$(http GET "catalogo/_design/validacion")
REV=$(cuerpo "$RESPUESTA_DDOC" | python3 -c \
  'import json,sys
try:
    print(json.load(sys.stdin).get("_rev", ""))
except Exception:
    print("")')
VALIDACION=$(python3 - "$REV" <<'PY'
import json, sys
from pathlib import Path
rev = sys.argv[1]
js = Path("infra/couchdb/validacion/catalogo.js").read_text(encoding="utf-8")
doc = {"_id": "_design/validacion", "validate_doc_update": js}
if rev:
    doc["_rev"] = rev
print(json.dumps(doc))
PY
)
RESPUESTA=$(http PUT "catalogo/_design/validacion" "$VALIDACION")
ESTADO=$(codigo "$RESPUESTA")
if [ "$ESTADO" = "201" ] || [ "$ESTADO" = "202" ]; then
  echo "  validación: instalada"
else
  echo "  validación: error $ESTADO: $(cuerpo "$RESPUESTA")" >&2
  exit 1
fi

echo "5. Vistas e índices"
./scripts/couchdb-ventas.sh >/dev/null
./scripts/couchdb-productos.sh >/dev/null
for INDICE in \
  infra/couchdb/descartes/indice_descartes_fecha.json \
  infra/couchdb/vencimientos/indice_vencimientos.json; do
  RESPUESTA=$(http POST "panaderia/_index" "$(cat "$INDICE")")
  ESTADO=$(codigo "$RESPUESTA")
  if [ "$ESTADO" = "200" ] || [ "$ESTADO" = "201" ]; then
    echo "  índice $(basename "$INDICE"): listo"
  else
    echo "  índice $(basename "$INDICE"): error $ESTADO: $(cuerpo "$RESPUESTA")" >&2
    exit 1
  fi
done

echo "6. Catálogo y costos"
./scripts/sembrar-catalogo.sh

echo "7. Prueba de permisos"
./infra/couchdb/prueba-permisos.sh
