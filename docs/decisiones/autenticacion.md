# Decisión: autenticación y login

## Contexto

La fase 4B del contrato exige login real con JWT, dos roles (`cajero` y
`encargado`), permisos distintos y una bandera para poder desarrollar sin login.
El frontend ya tenía `VITE_AUTH=activado`, pero no existía pantalla de login ni
se enviaba el encabezado `Authorization`. En la práctica, con el backend en
`AUTH_MODE=activo`, las ventas pendientes fallaban con `401`.

## Decisión

Se mantiene la autenticación real del contrato:

- `POST /auth/login` valida usuario y contraseña contra `usuarios/{usuario}`.
- La contraseña solo se guarda como hash **Argon2id**.
- El JWT se firma con HS256 y dura **12 horas**.
- Las rutas protegidas exigen `Authorization: Bearer <token>`.
- `cajero` puede registrar ventas; `encargado` además puede hornadas, cierre y
  reportes.
- `AUTH_MODE=desactivado` sigue disponible para desarrollo y usa un usuario
  falso con rol `encargado`. No se debe usar en producción.

En el navegador:

- El JWT se guarda en `localStorage` (`panaderia.sesion`). Nunca se guarda la
  contraseña ni se escribe el token en PouchDB.
- Al cargar la app se valida `expira_en`; una sesión vencida se descarta.
- La cola de ventas envía el token. Ante `401` vuelve al login y conserva las
  ventas pendientes. Ante `403` muestra un aviso y también las conserva.
- `VITE_AUTH=desactivado` permite entrar sin login y no muestra el botón Salir.

## Usuarios de aplicación

`infra/couchdb/setup.sh` crea o actualiza dos usuarios:

| Usuario | Rol | Variables |
|---|---|---|
| `cajero1` | `cajero` | `CAJERO_USUARIO`, `CAJERO_PASSWORD` |
| `encargado1` | `encargado` | `ENCARGADO_USUARIO`, `ENCARGADO_PASSWORD` |

Las contraseñas solo viven en `.env` (nunca en Git). El binario
`src/bin/sembrar_usuarios.rs` calcula el hash y conserva los demás campos del
documento si ya existía.

## Criterios de aceptación

- Credenciales válidas → `200` con token, `tipo`, `expira_en` y `usuario`.
- Credenciales inválidas → `401`.
- Token ausente, inválido o vencido → `401`.
- `cajero` en `POST /hornadas`, `POST /cierre` o reportes → `403`.
- `encargado` en esas rutas → permitido.
- `AUTH_MODE=desactivado` no exige token y usa rol `encargado`.
- Las pruebas de integración contra CouchDB real cubren estos casos en
  `tests/integracion.rs`.
- El frontend puede leer el catálogo: CouchDB permite el origen en
  `infra/couchdb/local.d/10-cors.ini`; la API lo permite en `CORS_ORIGINS`.

## Puesta en marcha en el servidor

1. Actualizar el código y revisar `.env` (nunca versionarlo):
   - `COUCHDB_USER`, `COUCHDB_PASSWORD`, `COUCHDB_URL`, `COUCHDB_PORT`
   - `BACKEND_USER=backend`, `BACKEND_PASSWORD`
   - `CLIENTE_PASSWORD`
   - `CAJERO_USUARIO`, `CAJERO_PASSWORD`
   - `ENCARGADO_USUARIO`, `ENCARGADO_PASSWORD`
   - `AUTH_MODE=activo`
   - `JWT_SECRET` largo y aleatorio
   - `CORS_ORIGINS=https://dominio-del-frontend` (separar varios con coma)
2. Levantar CouchDB y sembrar:
   ```bash
   docker compose --env-file .env -f infra/docker-compose.yml up -d --build couchdb
   ./infra/couchdb/setup.sh
   ```
3. Construir y levantar la API y Caddy:
   ```bash
   docker compose --env-file .env -f infra/docker-compose.yml up -d --build api caddy
   ```
4. Comprobar:
   ```bash
   curl -fsS https://panaderia-api.duckdns.org/salud
   curl -X POST https://panaderia-api.duckdns.org/auth/login \
     -H 'Content-Type: application/json' \
     -d '{"usuario":"cajero1","contrasena":"<CAJERO_PASSWORD>"}'
   ```
5. En el frontend, definir `VITE_API_URL=https://panaderia-api.duckdns.org` y
   `VITE_AUTH=activado`, y reconstruir la PWA.

Si el login no se pudiera estabilizar, el plan de respaldo es
`AUTH_MODE=desactivado` y `VITE_AUTH=desactivado`, documentado como modo de
desarrollo. No es la configuración recomendada para el servidor.
