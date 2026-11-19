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
   Define códigos de usuario de 3 dígitos, PIN numéricos de 4 dígitos y un
   `JWT_SECRET` largo. Nunca subas `.env`.
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
5. Comprueba `GET http://localhost:5000/salud` y el login:
   ```bash
   curl -X POST http://localhost:5000/auth/login \
     -H 'Content-Type: application/json' \
     -d '{"usuario":"101","contrasena":"<CAJERO_PASSWORD>"}'
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

### Actualizar una instalación existente

Desde la raíz del repositorio en el servidor Ubuntu, conserva el archivo
`.env` y ejecuta:

```bash
git pull --ff-only origin main &&
  docker compose --env-file .env -f infra/docker-compose.yml up -d --build --no-deps api &&
  curl -fsS --retry 10 --retry-connrefused --retry-delay 2 https://panaderia-api.duckdns.org/salud
```

Esta actualización reconstruye la API y conserva los contenedores de CouchDB
y Caddy, junto con sus volúmenes. Las opciones de importación de facturas
son opcionales y se configuran en el mismo `.env`.

## Calidad del código

La integración continua verifica formato, Clippy, datos canónicos, pruebas
unitarias y pruebas de integración contra un CouchDB de servicio.

## Trabajo colaborativo

Los cambios se proponen mediante Pull Requests hacia `main` y deben cumplir las
reglas de revisión y las comprobaciones automatizadas del repositorio.

## Inventario de producción

En **Producción → Recetas**, abre un producto existente o crea uno nuevo.
Cada material se identifica por el mismo `sku` que aparece en la factura.
Introduce en la receta cuánto consume **una unidad de producto**, medido en
la unidad que se factura. Por ejemplo, si se factura un saco de 50 kg con
`sku: harina-50kg`, 50 g por pan equivalen a 0.001 sacos por pan.

El documento del producto en CouchDB guarda la receta con cantidades enteras
en millonésimas para evitar redondeos:

```json
{
  "_id": "producto:pan-integral",
  "tipo": "producto",
  "nombre": "Pan integral",
  "precio_centavos": 75,
  "vida_util_dias": 2,
  "activo": true,
  "receta": [
    { "sku": "harina-50kg", "millonesimas": 1000 }
  ]
}
```

**Producción → Materiales** muestra las unidades facturadas menos el consumo
de las hornadas. La pantalla se actualiza cada 30 segundos y al registrar una
hornada. **Facturas** permite revisar los comprobantes y sus líneas.

Si faltan materiales, la API rechaza la hornada salvo que el encargado seleccione
**Registrar con faltantes**. Esa hornada deja un saldo negativo. La siguiente
factura del mismo SKU suma su cantidad y compensa el saldo pendiente.
Las facturas existentes se incluyen en el cálculo. Las hornadas históricas sin
receta guardada conservan su historial sin imputarles cantidades por estimación.
Antes de registrar una hornada nueva, configura la receta del producto.

El despliegue con Docker Compose utiliza una instancia de la API. Esa instancia
serializa el registro de hornadas para que dos solicitudes no gasten el mismo
saldo simultáneamente.

## Importar facturas desde Gmail

El backend puede revisar cada minuto un buzón dedicado con Gmail API. Crea en
ese buzón la etiqueta `facturas-panaderia` y aplícala al correo de prueba.
Solo se consultan correos de la dirección configurada en
`FACTURAS_GMAIL_REMITENTE` que tengan esa etiqueta y un adjunto
`factura.json`. Aplica la etiqueta a correos cuya procedencia hayas verificado.

Envía exactamente un adjunto con ese nombre, de hasta 16 KiB. Este es el
contrato inicial:

```json
{
  "numero": "FAC-0001",
  "proveedor": "Molinos del Sur",
  "fecha": "2026-11-18",
  "lineas": [
    { "sku": "harina-50kg", "descripcion": "Harina de trigo", "cantidad": 4, "costo_unitario": 28.5 },
    { "sku": "azucar-25kg", "descripcion": "Azúcar", "cantidad": 2, "costo_unitario": 21.0 }
  ]
}
```

Todos los campos son obligatorios y se rechazan campos adicionales. El total
se calcula sumando los subtotales de las líneas. Envía al menos una línea con
cantidad entera positiva y costo unitario positivo en USD, con hasta dos
decimales y un máximo de USD 10 000 000 000 por unidad. La API devuelve también `total_centavos` (15600 en este ejemplo).
Usa la misma escritura del nombre del proveedor en cada envío. En CouchDB, el proveedor y el número identifican una factura.
Un reenvío idéntico no la duplica. Si ya existe esa combinación con otros datos,
el worker registra el conflicto y conserva la factura guardada.

1. Habilita la [Gmail API](https://developers.google.com/workspace/guides/enable-apis)
   en un proyecto de Google Cloud.
2. Autoriza el buzón con un cliente OAuth 2.0 y solicita acceso sin conexión
   con el permiso `https://www.googleapis.com/auth/gmail.readonly`.
   Consulta cómo obtener el refresh token en la
   [guía de OAuth de Google](https://developers.google.com/identity/protocols/oauth2/web-server#offline).
   Para una prueba, también puedes usar
   [OAuth 2.0 Playground](https://developers.google.com/oauthplayground/)
   con tus propias credenciales. Ese flujo requiere un cliente web con
   `https://developers.google.com/oauthplayground` como URI de redirección.
3. Guarda en el `.env` local el buzón, el remitente, el ID y secreto del
   cliente OAuth y el refresh token, usando las variables
   `FACTURAS_GMAIL_*` de `.env.example`. Configura
   `FACTURAS_GMAIL_ACTIVO=true` y reinicia el backend con `cargo run`
   o vuelve a levantar el servicio `api` con Docker Compose.
4. Envía el correo de prueba, aplícale la etiqueta y consulta las facturas
   con el token JWT de un encargado:

   ```bash
   curl -fsS http://localhost:5000/facturas \
     -H "Authorization: Bearer $TOKEN_ENCARGADO"
   ```

   La respuesta contiene `facturas` (hasta 50 registros) y
   `siguiente_offset`. Si el offset no es `null`, solicita la siguiente
   página con `GET /facturas?offset=<siguiente_offset>`.

### Alternativa con Google Apps Script

Copia [el archivo del worker](scripts/facturas-apps-script.js) en `Code.gs` de
un proyecto creado desde el Gmail de facturas. En **Servicios**, agrega
**Gmail API**. Con el proyecto de Cloud predeterminado, Google habilita ese
servicio al agregarlo desde Apps Script.

En el servidor, genera una clave con `openssl rand -hex 32` y guárdala como
`FACTURAS_IMPORTACION_CLAVE` en el `.env`. Reinicia el backend para servir
`POST /facturas/importar`. En **Configuración del proyecto → Propiedades del
script**, agrega esa misma clave, `API_URL` con la base HTTPS pública del
backend y `REMITENTE` con la dirección que envía los adjuntos.

Ejecuta manualmente `importarFacturas` y autoriza el acceso desde el buzón
dedicado. Envía y etiqueta un correo de prueba. La etiqueta
`facturas-importadas` se aplica después de que el backend guarda la factura
o confirma un duplicado idéntico. Cuando la prueba funcione, crea un
activador de tiempo para `importarFacturas`, por ejemplo cada cinco minutos.
