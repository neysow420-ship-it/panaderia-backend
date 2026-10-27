# Pruebas de integración con CouchDB

Las pruebas de `tests/integracion.rs` arrancan el binario real de la API en un
puerto libre y usan un CouchDB real. No se ejecutan con un `cargo test` normal:
si falta `PANADERIA_INTEGRACION=1`, se omiten para no necesitar Docker en cada
prueba unitaria.

## Requisitos

- Docker con CouchDB levantado.
- `.env` con `COUCHDB_URL`, `COUCHDB_USER`, `COUCHDB_PASSWORD`,
  `BACKEND_PASSWORD`, `CAJERO_PASSWORD` y `ENCARGADO_PASSWORD`.
- Rust estable.

## Ejecutar

```bash
docker compose --env-file .env -f infra/docker-compose.yml up -d couchdb
./tests/ejecutar-integracion.sh
```

El lanzador:

1. Carga `.env`.
2. Comprueba que CouchDB responde.
3. Ejecuta `infra/couchdb/setup.sh` (salvo `PANADERIA_SALTAR_SETUP=1`).
4. Lanza `cargo test --test integracion -- --test-threads=1`.

Si `cargo` no está en el host, los scripts de siembra usan Docker. Para usar otro
archivo de entorno:

```bash
PANADERIA_ENV_FILE=/ruta/a/mi.env ./tests/ejecutar-integracion.sh
```

## Qué cubre

- `/salud` responde y CouchDB está disponible.
- Login correcto, credenciales incorrectas y respuesta con token/rol.
- Token ausente, inválido y expirado → `401`.
- Permisos: `cajero` no puede hornadas ni reportes; `encargado` sí.
- `AUTH_MODE=desactivado` no exige token.
- Venta idempotente: `201`, reenvío `200`, mismo ULID con otro contenido `409`.
- Cálculo en centavos con el precio del catálogo.
- Validaciones → `422`.
- Venta de un día cerrado: conserva `fecha_original` y se asigna a un día abierto.
- CORS para el origen configurado.
- Vista y índices Mango de ventas y hornadas instalados.

Cada prueba usa usuarios y ULID únicos; al terminar borra sus documentos. Las
pruebas se ejecutan en serie para no compartir estado de CouchDB.

## CI

`.github/workflows/ci.yml` levanta CouchDB como servicio, ejecuta la siembra y
lanza las pruebas unitarias y de integración. Las pruebas de datos canónicos se
ejecutan con:

```bash
python3 -m unittest discover -s scripts/pruebas
```
