# ADR: Sincronización offline-first

- **Fecha:** 2026-10-22
- **Estado:** Propuesta para congelar con `contrato-v1`
- **Responsable:** coordinador
- **Relacionado:** `contratos/documentos.md` §1.6–1.7, `contratos/openapi.yaml`,
  ítems 3.7, 4.11–4.15

## Contexto

La caja debe seguir vendiendo sin internet. PouchDB guarda la venta en el
dispositivo y un servicio la envía a la API Rust cuando hay conexión. El
servidor es la única fuente de verdad: una venta local **no es oficial** hasta
que CouchDB confirma la escritura. Hay que evitar duplicados, pérdidas y
reintentos infinitos, y distinguir un rechazo de negocio de un problema de red.

## Decisión

### 1. Dos bases locales separadas

| Base local | Contenido | Replicación |
|---|---|---|
| `pendientes` | ventas creadas en el dispositivo y su estado | **ninguna** (solo la usa el servicio de envío) |
| `espejo` | copia de solo lectura de `catalogo` | bajada desde CouchDB, `live: true`, `retry: true` |

Reglas:

- **Nunca** se replica `pendientes` hacia CouchDB ni hacia otro dispositivo.
- **Nunca** se replica `espejo` hacia arriba. El catálogo solo lo modifica el
  `backend` desde la API administrativa.
- `pendientes` no guarda tokens, contraseñas ni datos de otros usuarios.
- La PWA puede borrar `espejo` y volver a replicarlo; `pendientes` no se borra
  automáticamente.

### 2. Documento de `pendientes`

```json
{
  "_id": "pendiente:01J9ZK3M8Q4V7T2R5N6X8B0C1D",
  "tipo": "pendiente_venta",
  "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C1D",
  "dispositivo_id": "caja-1",
  "creado_local": "2026-10-13T08:15:32-06:00",
  "lineas": [{ "producto_id": "producto:pan-frances", "cantidad": 6 }],
  "estado": "pendiente",
  "intentos": 0,
  "proximo_intento_en": "2026-10-13T08:15:32-06:00",
  "ultimo_error": null,
  "requiere_revision": false,
  "venta_oficial_id": null,
  "creado_en": "2026-10-13T08:15:32-06:00",
  "actualizado_en": "2026-10-13T08:15:32-06:00"
}
```

- `_id` local: `pendiente:{ULID}`. El ULID es el mismo `operacion_id` que se
  envía a la API.
- `lineas` solo lleva producto y cantidad, igual que `SolicitudVenta`.
- `venta_oficial_id` se llena cuando el servidor responde `201` o `200`.
- `requiere_revision` se pone en `true` ante un `409`.

### 3. Estados

| Estado | Significado | Transiciones |
|---|---|---|
| `pendiente` | esperando su turno o reintento | → `enviando` |
| `enviando` | hay una petición en curso | → `aceptada`, `rechazada`, `pendiente` |
| `aceptada` | el servidor confirmó (`201` o `200`) | final |
| `rechazada` | `422`, `409` o `403`; requiere acción humana | final hasta corregir o descartar |

- `enviando` es un estado **transitorio**. Al iniciar la aplicación, cualquier
  documento que quedó en `enviando` (por un cierre abrupto) vuelve a
  `pendiente`, se reinicia el backoff y se reintenta.
- Un `409` se guarda como `rechazada` con
  `requiere_revision: true` y `ultimo_error.codigo = "operacion_en_conflicto"`.
  **No es éxito** y no se reintenta solo.
- Corregir una venta rechazada crea un **ULID nuevo** y un documento nuevo en
  `pendientes`. El original queda como `rechazada` para auditoría; no se
  reutiliza su ULID.
- Descartar una rechazada la marca como `descartada` (estado terminal local) y
  no la envía.

### 4. Servicio de envío

1. Lee como máximo un documento `pendiente` cuyo `proximo_intento_en` ya pasó.
2. Lo pasa a `enviando` con una marca de tiempo (lease).
3. Hace `POST /ventas` con el **ULID original** y exactamente las mismas
   líneas, caja y `creado_local`.
4. Interpreta la respuesta:

| Respuesta | Acción |
|---|---|
| `201` | `aceptada`; guarda `venta_oficial_id` |
| `200` | `aceptada`; `ya_procesada: true` |
| `422` | `rechazada`; guarda `errores[]` para la pantalla |
| `401` | pausa la cola y pide iniciar sesión; **no** cambia el estado |
| `403` | `rechazada`; avisa que el rol no permite vender |
| `409` | `rechazada` con `requiere_revision: true`; no es éxito |
| `503` o error de red | vuelve a `pendiente` con backoff |
| otro `5xx` | vuelve a `pendiente` con backoff |

5. El backoff es exponencial con jitter: 1 s, 2 s, 4 s, 8 s… hasta un máximo de
   5 minutos. El jitter evita que muchas cajas reintenten a la vez.
6. Solo hay un envío en curso por dispositivo. Si el lease vence (por ejemplo,
   2 minutos), el documento vuelve a `pendiente`.

### 5. Idempotencia

- El ULID se genera **al confirmar la venta en la caja**, no al enviarla.
- Un reintento después de una respuesta perdida usa el mismo ULID y la misma
  huella; el servidor responde `200` y la venta no se duplica.
- Si el servidor responde `409`, el contenido local no coincide con el que ya
  está guardado. No se reintenta: se marca para revisión y se corrige con un
  ULID nuevo.
- La huella se calcula igual que en `contratos/documentos.md` §1.6. La PWA
  puede calcularla para diagnostico, pero **el servidor la recalcula** y es la
  que manda.

### 6. Catálogo (`espejo`)

- Replicación de solo bajada: `PouchDB.replicate(remotoCatalogo, espejo, { live: true, retry: true })`.
- Nunca se configura la réplica inversa.
- Si no hay catálogo cacheado y es la primera conexión, la pantalla de venta
  avisa que no se puede vender hasta descargarlo.
- Si el catálogo cambia en el servidor, la réplica actualiza `espejo`; una venta
  pendiente conserva las líneas y cantidades originales, pero el precio oficial
  lo pone Rust al aceptar.
- El espejo **no** contiene costos ni usuarios.

### 7. Autenticación

- El token JWT se guarda en `localStorage` (o almacenamiento seguro del
  navegador), **nunca** dentro de un documento de PouchDB.
- Un `401` pausa la cola, conserva todos los `pendientes` y lleva a la pantalla
  de login. Al volver a iniciar sesión se reanuda.
- Si `AUTH_MODE=desactivado`, el servicio envía sin token y el servidor asigna
  el rol `encargado`; la cola funciona igual.
- No se registran tokens ni contraseñas en consola, logs ni documentos.

### 8. Diagrama

```text
Caja (PWA)
  ├── venta rápida ──► PouchDB `pendientes` (estado: pendiente)
  │                         │
  │                         ▼
  │                    servicio de envío ──POST /ventas──► Rust ──► CouchDB
  │                         │                                   │
  │                         └── 201/200: aceptada ◄─────────────┘
  │                             422/409/403: rechazada
  │                             401: pausa y login
  │                             503/red: pendiente + backoff
  │
  └── catálogo ◄── réplica de bajada ── CouchDB `catalogo` ──► PouchDB `espejo`
```

## Alternativas descartadas

| Alternativa | Motivo |
|---|---|
| Replicar `pendientes` directamente con PouchDB → CouchDB | CouchDB aceptaría documentos sin validar precios, fechas ni permisos; además el `409` y la idempotencia quedarían fuera del servidor |
| Guardar las ventas pendientes en `localStorage` | No es transaccional, se pierde con el almacenamiento del navegador y no permite consultas |
| Reintentar un `409` con el mismo ULID | Nunca va a funcionar: el contenido ya es distinto; hay que corregir con ULID nuevo |
| Marcar `aceptada` con un `409` | Un `409` significa que la venta no se guardó; contarla como éxito perdería la venta |
| Replicación bidireccional del catálogo | La PWA podría cambiar precios y afectar los importes oficiales |
| Borrar `pendientes` al cerrar sesión | Perdería ventas no enviadas; la cola pertenece al dispositivo, no a la sesión |

## Consecuencias

- La caja puede vender sin internet y la venta no es oficial hasta que Rust
  confirma.
- El servidor sigue siendo la única fuente de verdad de precios, fechas y
  merma.
- Un `409` requiere intervención humana; la pantalla de rechazadas debe mostrar
  el motivo y permitir corregir con un ULID nuevo o descartar.
- El servicio de envío debe probarse con respuestas perdidas, doble envío,
  `401`, `409`, `422` y `503` (ítems 4.13–4.15 y 6.5).
- La réplica de bajada mantiene el catálogo actualizado sin exponer costos.

## Criterios de aceptación

1. Al recargar o cerrar y reabrir la PWA, `pendientes` conserva las ventas.
2. Al iniciar, ningún documento queda en `enviando`.
3. Un doble envío del mismo ULID produce una sola venta oficial y la segunda
   respuesta es `200`.
4. Un `409` no se marca como `aceptada`.
5. Corregir una rechazada genera un ULID distinto.
6. `espejo` se actualiza con `live` y `retry`, y no existe réplica ascendente.
7. Un `401` no borra la cola.
