# CouchDB: bases, usuarios y permisos

Este directorio contiene la configuración de CouchDB para la fase 3.8:

| Base | Uso | `backend` | `cliente` |
|---|---|---|---|
| `panaderia` | ventas, hornadas, descartes, cierres, `config:vigente` | leer/escribir | sin acceso |
| `catalogo` | `producto:{slug}` (precio de venta) | leer/escribir | **solo lectura** |
| `usuarios` | `usuario:{nombre}` con hash argon2 | leer/escribir | sin acceso |

`backend` es administrador de las tres bases (rol `backend`). `cliente` solo es
miembro de `catalogo` (rol `cliente`). Ninguno es administrador del servidor:
el administrador del servidor se crea desde `COUCHDB_USER` y
`COUCHDB_PASSWORD`.

## 1. Preparar el entorno

```bash
cp .env.example .env
# Edita .env y define, como mínimo:
#   COUCHDB_USER, COUCHDB_PASSWORD, COUCHDB_URL, COUCHDB_PORT
#   BACKEND_PASSWORD, CLIENTE_PASSWORD
docker compose --env-file .env -f infra/docker-compose.yml up -d
```

Espera a que el contenedor esté `healthy`:

```bash
docker compose --env-file .env -f infra/docker-compose.yml ps
```

## 2. Configurar bases, usuarios y permisos

```bash
./infra/couchdb/setup.sh
```

El script es idempotente y hace lo siguiente:

1. Crea `panaderia`, `catalogo`, `usuarios` y `_users` si no existen.
2. Crea o actualiza `org.couchdb.user:backend` (rol `backend`) y
   `org.couchdb.user:cliente` (rol `cliente`).
3. Aplica `_security` a cada base.
4. Instala `validate_doc_update` en `catalogo` desde
   `infra/couchdb/validacion/catalogo.js`.
5. Siembra los 5 productos y sus costos con `scripts/sembrar-catalogo.sh`.
6. Ejecuta `prueba-permisos.sh`.

## 3. Prueba de permisos

```bash
./infra/couchdb/prueba-permisos.sh
```

Comprueba:

- `cliente` lee `catalogo` → `200`.
- `cliente` intenta escribir `catalogo` → `403`.
- `cliente` lee `panaderia` → `401` o `403`.
- `cliente` lee `usuarios` → `401` o `403`.
- `backend` escribe `catalogo` → `201`.
- La validación rechaza un precio negativo → `403`.
- `backend` borra el documento de prueba → `200`.

También se puede probar a mano:

```bash
source .env
curl -u "cliente:$CLIENTE_PASSWORD" "$COUCHDB_URL/catalogo/_all_docs"
curl -u "cliente:$CLIENTE_PASSWORD" "$COUCHDB_URL/panaderia/_all_docs"   # debe fallar
curl -u "backend:$BACKEND_PASSWORD" "$COUCHDB_URL/catalogo/_all_docs"
```

## 4. Reglas de `validate_doc_update`

`infra/couchdb/validacion/catalogo.js`:

- Solo `backend` (o un administrador del servidor) puede escribir.
- `cliente` puede leer, pero no crear, modificar ni borrar.
- Solo se aceptan documentos `producto:{slug}`.
- `nombre` no vacío, `precio_centavos` entero ≥ 0, `vida_util_dias` entero
  entre 1 y 365, `activo` booleano.
- Los design docs solo los administra `backend`.

## 5. Reiniciar desde cero

```bash
docker compose --env-file .env -f infra/docker-compose.yml down -v
docker compose --env-file .env -f infra/docker-compose.yml up -d
./infra/couchdb/setup.sh
```

`down -v` borra el volumen `couchdb_data` y, con él, las bases y usuarios.
