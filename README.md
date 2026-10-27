# Sistema de gestión para panaderías — Backend

API y lógica de negocio de un sistema de gestión para panaderías, desarrollado
como proyecto académico de Ingeniería de Sistemas.

## Objetivo

Desarrollar un backend que permita validar y procesar las operaciones del
sistema, con énfasis en la consistencia de los datos y el funcionamiento en
entornos donde la conectividad puede ser limitada.

## Tecnologías

- **Rust:** lógica de negocio, validaciones y servicios de la API.
- **Axum:** framework para construir la API HTTP.
- **Apache CouchDB:** persistencia central de documentos JSON.
- **Docker Compose:** entorno reproducible de servicios.
- **JWT + Argon2id:** autenticación y almacenamiento de contraseñas.

## Arquitectura

El backend recibe las solicitudes del cliente, valida las operaciones y determina
qué información puede persistirse en CouchDB.

Flujo general:

`Frontend → PouchDB → API Rust → Validación → CouchDB`

Los cambios locales pendientes de sincronización se envían a la API con el JWT
del usuario. La cola local conserva las ventas si falla la red o si la sesión
vence.

## Estado

Fase 4 en desarrollo. Ya están implementados:

- Catálogo canónico de 5 productos y costos internos.
- Ventas idempotentes, hornadas, descartes, vencimientos, cierres y reportes.
- Login con JWT de 12 horas y permisos por rol (`cajero`, `encargado`).
- Siembra de usuarios con contraseñas desde variables de entorno.
- Vistas e índices Mango en CouchDB.
- Pruebas unitarias y de integración contra CouchDB real.

## Desarrollo local

1. Copia la plantilla y edítala:
   ```bash
   cp .env.example .env
   ```
   Define las contraseñas y un `JWT_SECRET` largo. Nunca subas `.env`.
2. Levanta CouchDB (incluye CORS para la réplica del catálogo):
   ```bash
   docker compose --env-file .env -f infra/docker-compose.yml up -d --build couchdb
   ```
3. Siembra bases, catálogo, vistas y usuarios:
   ```bash
   ./infra/couchdb/setup.sh
   ```
4. Levanta la API:
   ```bash
   cargo run
   ```
   o con Docker:
   ```bash
   docker compose --env-file .env -f infra/docker-compose.yml up -d --build api caddy
   ```
5. Comprueba `GET http://localhost:3000/salud` y el login:
   ```bash
   curl -X POST http://localhost:3000/auth/login \
     -H 'Content-Type: application/json' \
     -d '{"usuario":"cajero1","contrasena":"<CAJERO_PASSWORD>"}'
   ```

Instrucciones detalladas: `infra/read.md`, `infra/couchdb/README.md` y
`docs/decisiones/autenticacion.md`.

## Pruebas

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 -m unittest discover -s scripts/pruebas
./tests/ejecutar-integracion.sh
```

Las pruebas de integración necesitan CouchDB real y se documentan en
`docs/pruebas-integracion.md`.

## Servidor

La API se publica en `https://panaderia-api.duckdns.org` mediante Caddy y Docker
Compose. En el servidor deben configurarse `AUTH_MODE=activo`, `JWT_SECRET`,
`CORS_ORIGINS` con el dominio del frontend y las contraseñas de los usuarios de
aplicación. El procedimiento está en `docs/decisiones/autenticacion.md`.

## Calidad del código

La integración continua verifica formato, Clippy, datos canónicos, pruebas
unitarias y pruebas de integración contra un CouchDB de servicio.

## Trabajo colaborativo

Los cambios se proponen mediante Pull Requests hacia `main` y deben cumplir las
reglas de revisión y las comprobaciones automatizadas del repositorio.
