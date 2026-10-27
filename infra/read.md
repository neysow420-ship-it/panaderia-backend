## Levantar CouchDB y la API (entorno local)

Requisitos: Docker con el motor iniciado.

1. Copia la plantilla de variables y edítala:
   - PowerShell: `Copy-Item .env.example .env`
   - Define `COUCHDB_PASSWORD`, `BACKEND_PASSWORD`, `CLIENTE_PASSWORD`,
     `CAJERO_PASSWORD` y `ENCARGADO_PASSWORD`.
   - Define un `JWT_SECRET` largo y aleatorio.
   - Si el puerto 5984 está ocupado, cambia `COUCHDB_PORT` (por ejemplo a
     15984) y ajusta `COUCHDB_URL` al mismo puerto.
   - `CORS_ORIGINS` debe incluir el origen del frontend.
2. Levanta solo CouchDB (la primera vez compila la imagen con la
   configuración de CORS):
   `docker compose --env-file .env -f infra/docker-compose.yml up -d --build couchdb`
3. Comprueba el estado (debe decir `Up (healthy)`):
   `docker compose --env-file .env -f infra/docker-compose.yml ps`
4. Crea las bases, usuarios, permisos, vistas, catálogo y usuarios de
   aplicación:
   `./infra/couchdb/setup.sh`
5. Levanta la API y Caddy:
   `docker compose --env-file .env -f infra/docker-compose.yml up -d --build api caddy`
6. Comprueba la API:
   `curl http://localhost:3000/salud`
7. Abre el panel de CouchDB: `http://localhost:<COUCHDB_PORT>/_utils` e inicia
   sesión con tu usuario y contraseña del `.env`.
8. Apagar (conserva los datos): `docker compose --env-file .env -f infra/docker-compose.yml down`

El archivo `.env` nunca se sube a GitHub.

### CORS del catálogo

El frontend lee el catálogo con PouchDB desde el navegador, así que CouchDB
también necesita CORS. La configuración está en
`infra/couchdb/local.d/10-cors.ini` y se copia dentro de la imagen de CouchDB.
Si el frontend se publica en otro dominio, agrega el origen en ese archivo y
reconstruye:

```bash
docker compose --env-file .env -f infra/docker-compose.yml up -d --build couchdb
```

No uses `*` para los orígenes: el navegador envía `Authorization` con las
credenciales de lectura del catálogo.

### Usar otro archivo de entorno

Los scripts de `scripts/` y `tests/` aceptan `PANADERIA_ENV_FILE`:

```bash
PANADERIA_ENV_FILE=/tmp/panaderia-local.env ./infra/couchdb/setup.sh
```

Esto permite probar con otro puerto de CouchDB sin tocar el `.env` principal.

### Servidor

En el servidor, `AUTH_MODE=activo`, `VITE_AUTH=activado` y `CORS_ORIGINS` con el
dominio real del frontend. Los pasos y el procedimiento de verificación están en
`docs/decisiones/autenticacion.md`.
