## Levantar CouchDB (entorno local)

Requisitos: Docker Desktop abierto (debe decir "Engine running").

1. Copia la plantilla de variables y edítala:
   - PowerShell: `Copy-Item .env.example .env`
   - Abre `.env` y pon tu propia contraseña en `COUCHDB_PASSWORD`.
   - Define también `BACKEND_PASSWORD` y `CLIENTE_PASSWORD` (usuarios de la
     aplicación; ver `infra/couchdb/README.md`).
   - Si el puerto 5984 está ocupado en tu equipo, cambia `COUCHDB_PORT` (por
     ejemplo a 15984) y ajusta `COUCHDB_URL` al mismo puerto.
2. Levanta CouchDB desde la raíz del repo:
   `docker compose --env-file .env -f infra/docker-compose.yml up -d`
3. Comprueba el estado (debe decir `Up (healthy)`):
   `docker compose --env-file .env -f infra/docker-compose.yml ps`
4. Crea las bases, usuarios, permisos y el catálogo:
   `./infra/couchdb/setup.sh`
5. Abre el panel: `http://localhost:<COUCHDB_PORT>/_utils` e inicia sesión con tu usuario y contraseña del `.env`.
6. Apagar (conserva los datos): `docker compose --env-file .env -f infra/docker-compose.yml down`

 El archivo `.env` nunca se sube a GitHub.