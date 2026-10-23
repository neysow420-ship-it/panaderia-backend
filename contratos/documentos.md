# Contrato de documentos (v1)

- **Estado:** propuesta para congelar como `contrato-v1` (ítem 3.6)
- **Ámbito:** bases `catalogo`, `panaderia` y `usuarios` de CouchDB
- **Fuente:** documento estándar del grupo (ODT), código de `src/ventas`,
  `src/productos`, `src/descartes`, `src/vencimientos` y checklist v4 (3.1–3.4)
- **API:** ver [`openapi.yaml`](openapi.yaml)

Este documento es la referencia única para Rust, la línea base Node/PostgreSQL,
el frontend y los scripts de siembra. Si el código y este documento discrepan,
el documento manda y el código se corrige.

---

## 1. Convenciones comunes

### 1.1 Bases y responsabilidades

| Base | Contenido | Quién escribe | Quién lee |
|---|---|---|---|
| `catalogo` | `producto:{slug}` | `backend` | `backend`, `cliente`, PWA (réplica de bajada) |
| `panaderia` | ventas, hornadas, descartes, cierres y `config:vigente` | `backend` | `backend` |
| `usuarios` | `usuario:{nombre}` con el hash de la contraseña | `backend` | `backend` |

Los **costos nunca viven en `catalogo`**: solo en `panaderia/config:vigente`.
El cliente (PWA) replica únicamente `catalogo`, así que nunca recibe costos.

### 1.2 Identificadores (`_id`)

| Documento | `_id` | Origen | ¿Inmutable? |
|---|---|---|---|
| Producto | `producto:{slug}` | servidor, desde el slug | sí |
| Configuración | `config:vigente` | fijo | el `_id` sí |
| Venta | `venta:{ULID}` | cliente (ULID de la operación) | sí |
| Hornada | `hornada:{ULID}` | cliente (se genera al abrir el formulario) | sí |
| Descarte | `descarte:{ULID}` | cliente o servidor | sí |
| Cierre | `cierre:{AAAA-MM-DD}` | servidor, desde la fecha de negocio | sí |
| Usuario | `usuario:{nombre}` | servidor | sí |
| Vistas e índices | `_design/{nombre}` | administrador | no |

Reglas:

- `slug` y `nombre` de usuario: minúsculas, `[a-z0-9-]`, sin espacios, de 1 a 64
  caracteres. El slug no se puede cambiar después de crear el producto.
- Un `_id` no se reutiliza para otro contenido. Para corregir una venta se crea
  un **ULID nuevo**; nunca se edita la venta original.
- No se usan `_local/*` ni IDs generados por CouchDB para documentos del
  negocio: el cliente debe poder reintentar con el mismo `_id`.
- Un `_id` que no cumpla la tabla se rechaza con `422`.

### 1.3 ULID

- 26 caracteres en base32 de Crockford: `0123456789ABCDEFGHJKMNPQRSTVWXYZ`.
- El primer carácter debe ser `0`–`7` (los primeros bits son la hora).
- Ejemplo válido: `01J9ZK3M8Q4V7T2R5N6X8B0C1D`.
- Ejemplo inválido: `01J9ZK3M8Q4V7T2R5N6X8B0C1` (25 caracteres) o
  `01J9ZK3M8Q4V7T2R5N6X8B0C1I` (contiene `I`).

El ULID es la **llave de idempotencia**: el mismo documento se reenvía con el
mismo `_id`. Dos operaciones distintas siempre llevan ULID distinto.

### 1.4 Fechas y zona horaria

- **Instantes** (`creado_local`, `recibido_servidor`, `creado_en`,
  `cerrado_en`, `actualizado_en`): RFC 3339 con desfase obligatorio, por
  ejemplo `2026-10-13T08:15:32-06:00`. No se aceptan fechas sin zona.
- **Fechas de negocio** (`fecha`, `fecha_original`, `vigente_desde`,
  `fecha_vencimiento`): `AAAA-MM-DD`.
- La zona del negocio es **`America/El_Salvador` (UTC-6, sin horario de
  verano)**. El servidor convierte todo instante a esa zona antes de calcular
  la fecha de negocio.
- `2026-10-14T01:30:00Z` pertenece al día de negocio `2026-10-13` (19:30 en
  El Salvador).
- El servidor escribe sus propios instantes con desfase `-06:00`.

### 1.5 Dinero

- Todo importe es un **entero de centavos** (`i64` en Rust, `BIGINT` en
  PostgreSQL). `10` significa $0.10.
- No se usan decimales, `float` ni cadenas para dinero.
- Los importes son no negativos. Las cantidades son enteros mayores que cero.
- Rust calcula `subtotal_centavos = cantidad × precio_centavos` y
  `total_centavos = Σ subtotales`, con aritmética comprobada (sin desbordes).

### 1.6 Huella (`huella`)

Cadena `sha256:<hex>` que identifica el **contenido de la operación**. Permite
distinguir un reenvío legítimo de un ULID reutilizado con otros datos.

Para una venta, el texto canónico es exactamente:

```text
v1|<operacion_id>|<dispositivo_id recortado>|<creado_local>|<linea 1><linea 2>...
```

donde cada línea es `producto_id=cantidad;` y las líneas se ordenan
lexicográficamente por `producto_id`. Después se aplica SHA-256 y se escribe en
hexadecimal minúsculas con el prefijo `sha256:`.

Ejemplo:

```text
v1|01J9ZK3M8Q4V7T2R5N6X8B0C1D|caja-1|2026-10-13T08:15:32-06:00|producto:donas=2;producto:pan-frances=6;
```

Notas:

- No se incluyen precios, usuario ni hora del servidor: no forman parte de lo
  que decidió el cliente y pueden variar entre reintentos.
- `creado_local` se usa tal como llegó; el cliente debe reenviar la misma
  cadena en cada reintento.
- Cambiar cualquier línea, la caja o la hora local cambia la huella.

### 1.7 Idempotencia y conflictos

| Situación | Respuesta |
|---|---|
| `_id` nuevo, datos válidos | `201` (o `200` en endpoints que solo aceptan idempotencia) |
| Mismo `_id` y misma `huella` | `200` con `ya_procesada: true` |
| Mismo `_id` y `huella` distinta | `409` con `codigo` específico |
| Datos inválidos | `422` con `{codigo, errores[]}` |
| Sin autenticación | `401` |
| Rol insuficiente | `403` |
| CouchDB no disponible | `503` |

La escritura se confirma en CouchDB **antes** de responder éxito. Un `409` no
es éxito y no se debe reintentar con el mismo `_id` y otro contenido.

### 1.8 Revisiones y campos desconocidos

- `_rev` lo asigna CouchDB. El cliente no lo envía al crear. Al actualizar un
  producto o la configuración, el servidor lee la `_rev` actual y la reenvía;
  una `_rev` vieja produce `409`.
- Los documentos `producto` y `config` **se extienden**: el servidor conserva
  los campos que no conoce al actualizar.
- En los documentos de eventos (venta, hornada, descarte, cierre) no se
  conservan campos ajenos: el contrato es cerrado.
- Un consumidor debe ignorar campos nuevos que no conozca; no debe fallar por
  ellos.

### 1.9 JSON

- Objetos JSON con nombres en `snake_case`.
- Los campos opcionales se omiten cuando no aplican; no se envían como `null`
  salvo que el contrato lo diga.
- No se permiten comentarios ni comas finales.

---

## 2. `producto` — base `catalogo`

Catálogo replicado por la PWA. Solo contiene precio de venta, nunca costos.

```json
{
  "_id": "producto:pan-frances",
  "_rev": "1-9f2c...",
  "tipo": "producto",
  "nombre": "Pan francés",
  "precio_centavos": 10,
  "vida_util_dias": 1,
  "activo": true
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | `producto:{slug}` |
| `_rev` | string | solo al leer/actualizar | la asigna CouchDB |
| `tipo` | string | sí | exactamente `producto` |
| `nombre` | string | sí | 1–80 caracteres, no vacío |
| `precio_centavos` | entero | sí | ≥ 0 |
| `vida_util_dias` | entero | sí | 1–365 |
| `activo` | booleano | sí | si es `false`, no se puede vender |

Campos desconocidos: se conservan (documento extensible).

### 2.1 Catálogo canónico v1 (5 productos)

| `_id` | Nombre | `precio_centavos` | `costo_unitario_centavos` | `vida_util_dias` |
|---|---|---:|---:|---:|
| `producto:pan-frances` | Pan francés | 10 | 5 | 1 |
| `producto:semita` | Semita | 25 | 12 | 2 |
| `producto:hojitas` | Hojitas | 15 | 8 | 2 |
| `producto:cachos` | Cachos | 30 | 15 | 2 |
| `producto:donas` | Donas | 35 | 18 | 2 |

El precio vive en `catalogo/producto:*`. El costo vive **solo** en
`panaderia/config:vigente`. La siembra reproducible está en
`scripts/datos/catalogo.json`, `scripts/datos/config.json` y
`scripts/sembrar-catalogo.sh`.

### 2.2 Ejemplos inválidos

```json
{ "_id": "producto:pan-frances", "tipo": "producto", "nombre": "", "precio_centavos": 10, "vida_util_dias": 1, "activo": true }
```
→ `422`: `nombre` vacío.

```json
{ "_id": "producto:pan-frances", "tipo": "producto", "nombre": "Pan", "precio_centavos": 0.10, "vida_util_dias": 1, "activo": true }
```
→ `422`: `precio_centavos` no es entero (el dinero va en centavos).

```json
{ "_id": "pan-frances", "tipo": "producto", "nombre": "Pan", "precio_centavos": 10, "vida_util_dias": 1, "activo": true }
```
→ `422`: el `_id` no tiene el prefijo `producto:`.

---

## 3. `config` — base `panaderia`

Configuración vigente del negocio. Es la **única** fuente de costos.

```json
{
  "_id": "config:vigente",
  "_rev": "3-a1b2...",
  "tipo": "config",
  "vigente_desde": "2026-10-13",
  "modo_produccion": "modelo",
  "alerta_dias": 1,
  "costos": [
    { "producto_id": "producto:pan-frances", "costo_unitario_centavos": 5 },
    { "producto_id": "producto:semita", "costo_unitario_centavos": 12 },
    { "producto_id": "producto:hojitas", "costo_unitario_centavos": 8 },
    { "producto_id": "producto:cachos", "costo_unitario_centavos": 15 },
    { "producto_id": "producto:donas", "costo_unitario_centavos": 18 }
  ]
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | exactamente `config:vigente` |
| `tipo` | string | sí | `config` |
| `vigente_desde` | fecha | sí | `AAAA-MM-DD` |
| `modo_produccion` | string | sí | `modelo` o `lotes` |
| `alerta_dias` | entero | sí | 0–30 |
| `costos` | arreglo | sí | un elemento por producto, sin repetidos |
| `costos[].producto_id` | string | sí | debe existir en `catalogo` |
| `costos[].costo_unitario_centavos` | entero | sí | ≥ 0 |

En v1 se usa `modo_produccion: "modelo"` (producción planificada por día, sin
arrastre de existencias). `lotes` queda reservado para la extensión P2 de
recetas e insumos; no cambia la fórmula del cierre.

Campos desconocidos: se conservan.

### 3.1 Ejemplo inválido

```json
{ "_id": "config:vigente", "tipo": "config", "vigente_desde": "2026-10-13", "modo_produccion": "modelo", "alerta_dias": 99, "costos": [] }
```
→ `422`: `alerta_dias` fuera de rango.

---

## 4. Solicitud de venta — `POST /ventas`

Es lo que envía el cliente. **No incluye precios, subtotales ni total.**

```json
{
  "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C1D",
  "dispositivo_id": "caja-1",
  "creado_local": "2026-10-13T08:15:32-06:00",
  "lineas": [
    { "producto_id": "producto:pan-frances", "cantidad": 6 },
    { "producto_id": "producto:donas", "cantidad": 2 }
  ]
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `operacion_id` | string | sí | ULID de 26 caracteres |
| `dispositivo_id` | string | sí | 1–64 caracteres, se recorta |
| `creado_local` | string | sí | RFC 3339 con zona; no más de 5 min en el futuro |
| `lineas` | arreglo | sí | 1–50 líneas |
| `lineas[].producto_id` | string | sí | `producto:{slug}` existente y activo |
| `lineas[].cantidad` | entero | sí | 1–1000 |

Reglas adicionales:

- Un producto no puede repetirse en dos líneas; se suman las cantidades en una.
- El servidor busca el precio en el catálogo cargado en memoria y **calcula**
  los importes.
- El `usuario` y la `fecha` los pone el servidor; si el cliente los manda, se
  ignoran.

### 4.1 Ejemplos inválidos

```json
{ "operacion_id": "abc", "dispositivo_id": "caja-1", "creado_local": "2026-10-13T08:15:32-06:00", "lineas": [{ "producto_id": "producto:pan-frances", "cantidad": 0 }] }
```
→ `422` por `operacion_id` y por `lineas[0].cantidad`.

```json
{ "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C1D", "dispositivo_id": "caja-1", "creado_local": "2026-10-13 08:15:32", "lineas": [{ "producto_id": "producto:pan-frances", "cantidad": 6 }] }
```
→ `422`: `creado_local` no es RFC 3339 con zona.

```json
{ "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C1D", "dispositivo_id": "caja-1", "creado_local": "2026-10-13T08:15:32-06:00", "lineas": [] }
```
→ `422`: la venta no tiene productos.

---

## 5. Venta oficial — base `panaderia`

Documento que guarda el servidor. Es la fuente de verdad para reportes y cierre.

```json
{
  "_id": "venta:01J9ZK3M8Q4V7T2R5N6X8B0C1D",
  "_rev": "1-7c3d...",
  "tipo": "venta",
  "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C1D",
  "huella": "sha256:6f1c...",
  "usuario": "cajero",
  "dispositivo_id": "caja-1",
  "creado_local": "2026-10-13T08:15:32-06:00",
  "recibido_servidor": "2026-10-13T08:15:40-06:00",
  "fecha": "2026-10-13",
  "lineas": [
    { "producto_id": "producto:pan-frances", "cantidad": 6, "precio_centavos": 10, "subtotal_centavos": 60 },
    { "producto_id": "producto:donas", "cantidad": 2, "precio_centavos": 35, "subtotal_centavos": 70 }
  ],
  "total_centavos": 130
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | `venta:{ULID}` |
| `tipo` | string | sí | `venta` |
| `operacion_id` | string | sí | igual al ULID del `_id` |
| `huella` | string | sí | ver §1.6 |
| `usuario` | string | sí | usuario autenticado (o `encargado` con auth desactivada) |
| `dispositivo_id` | string | sí | recortado |
| `creado_local` | string | sí | tal como llegó |
| `recibido_servidor` | string | sí | instante del servidor, `-06:00` |
| `fecha` | fecha | sí | día de negocio al que cuenta la venta |
| `fecha_original` | fecha | no | solo si el día original ya estaba cerrado |
| `lineas` | arreglo | sí | precios congelados del catálogo |
| `lineas[].precio_centavos` | entero | sí | precio vigente al aceptar |
| `lineas[].subtotal_centavos` | entero | sí | `cantidad × precio_centavos` |
| `total_centavos` | entero | sí | suma de subtotales |

`fecha` es la fecha que usan el cierre y los reportes. `fecha_original` solo
aparece en ventas tardías (ver `docs/decisiones/merma.md`).

### 5.1 Ejemplo de venta tardía

Venta hecha el 12 a las 19:50, enviada el 13 cuando el 12 ya estaba cerrado:

```json
{
  "_id": "venta:01J9ZK3M8Q4V7T2R5N6X8B0C2E",
  "tipo": "venta",
  "operacion_id": "01J9ZK3M8Q4V7T2R5N6X8B0C2E",
  "huella": "sha256:...",
  "usuario": "cajero",
  "dispositivo_id": "caja-1",
  "creado_local": "2026-10-12T19:50:00-06:00",
  "recibido_servidor": "2026-10-13T08:10:00-06:00",
  "fecha": "2026-10-13",
  "fecha_original": "2026-10-12",
  "lineas": [{ "producto_id": "producto:pan-frances", "cantidad": 6, "precio_centavos": 10, "subtotal_centavos": 60 }],
  "total_centavos": 60
}
```

### 5.2 Ejemplo inválido

Una venta oficial con `total_centavos` distinto de la suma de subtotales, o con
`operacion_id` distinto del ULID del `_id`, es inválida. El servidor nunca
confía en totales del cliente.

---

## 6. `hornada` — base `panaderia`

Registro de producción. Se crea solo con conexión y rol `encargado`.

```json
{
  "_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K",
  "_rev": "1-2b4e...",
  "tipo": "hornada",
  "huella": "sha256:...",
  "usuario": "encargado",
  "creado_en": "2026-10-13T05:30:00-06:00",
  "fecha": "2026-10-13",
  "producto_id": "producto:pan-frances",
  "unidades": 150,
  "fecha_vencimiento": "2026-10-14",
  "pronostico_id": "pronostico:2026-10-13"
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | `hornada:{ULID}` generado al abrir el formulario |
| `tipo` | string | sí | `hornada` |
| `huella` | string | sí | contenido canónico de la hornada |
| `usuario` | string | sí | encargado autenticado |
| `creado_en` | string | sí | RFC 3339 con zona |
| `fecha` | fecha | sí | día de negocio de `creado_en` |
| `producto_id` | string | sí | producto existente |
| `unidades` | entero | sí | 1–100000 |
| `fecha_vencimiento` | fecha | sí | ≥ `fecha`; por defecto `fecha + vida_util_dias` |
| `pronostico_id` | string | no | reservado para planificación |

Idempotencia: mismo ULID y misma huella → `200`; mismo ULID y otra huella →
`409`. El ULID se genera **al abrir el formulario** y se reutiliza en todos los
reintentos.

### 6.1 Ejemplo inválido

```json
{ "_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K", "tipo": "hornada", "fecha": "2026-10-13", "producto_id": "producto:pan-frances", "unidades": 0, "fecha_vencimiento": "2026-10-12" }
```
→ `422`: `unidades` debe ser > 0 y `fecha_vencimiento` no puede ser anterior a
`fecha`.

---

## 7. `descarte` — base `panaderia`

Unidades retiradas de un lote. En v1 el cierre puede declarar descartes por
producto; este documento permite el detalle por lote y motivo.

```json
{
  "_id": "descarte:01J9ZM1R7S8T9V0W1X2Y3Z4A5B",
  "_rev": "1-5d6f...",
  "tipo": "descarte",
  "fecha": "2026-10-13",
  "lote_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K",
  "producto_id": "producto:pan-frances",
  "unidades": 5,
  "motivo": "danado",
  "usuario": "encargado",
  "creado_en": "2026-10-13T19:40:00-06:00"
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | `descarte:{ULID}` |
| `tipo` | string | sí | `descarte` |
| `fecha` | fecha | sí | día de negocio del descarte |
| `lote_id` | string | sí | `hornada:{ULID}` existente |
| `producto_id` | string | sí | producto del lote |
| `unidades` | entero | sí | 1–100000 |
| `motivo` | string | sí | `danado`, `vencido` u `otro` |
| `usuario` | string | sí | encargado que lo registra |
| `creado_en` | string | sí | RFC 3339 con zona |

El descarte cuenta en el cierre como `descartadas` y se valoriza a costo.

### 7.1 Ejemplo inválido

```json
{ "_id": "descarte:01J9ZM1R7S8T9V0W1X2Y3Z4A5B", "tipo": "descarte", "fecha": "2026-10-13", "lote_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K", "producto_id": "producto:pan-frances", "unidades": -2, "motivo": "", "usuario": "encargado", "creado_en": "2026-10-13T19:40:00-06:00" }
```
→ `422`: `unidades` ≤ 0 y `motivo` vacío.

---

## 8. `cierre` — base `panaderia`

Resumen inmutable de un día de negocio. `_id` fijo `cierre:{fecha}`.

```json
{
  "_id": "cierre:2026-10-13",
  "_rev": "1-8a9b...",
  "tipo": "cierre",
  "fecha": "2026-10-13",
  "responsable": "encargado",
  "cerrado_en": "2026-10-13T20:00:00-06:00",
  "modo_produccion": "modelo",
  "confirmo_diferencia": true,
  "huella": "sha256:...",
  "por_producto": [
    {
      "producto_id": "producto:pan-frances",
      "existencia_inicial": 0,
      "producidas": 150,
      "vendidas": 128,
      "descartadas": 20,
      "diferencia": 2,
      "existencia_final": 2,
      "lotes_finales": [],
      "costo_aplicado_centavos": 5,
      "merma_centavos": 100,
      "merma_bp": 1333
    }
  ],
  "total_merma_centavos": 100
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | `cierre:{AAAA-MM-DD}` |
| `tipo` | string | sí | `cierre` |
| `fecha` | fecha | sí | día cerrado |
| `responsable` | string | sí | encargado que cierra |
| `cerrado_en` | string | sí | instante del servidor |
| `modo_produccion` | string | sí | copia de `config:vigente` |
| `confirmo_diferencia` | booleano | sí | `true` si hay diferencias positivas |
| `huella` | string | sí | contenido canónico del cierre |
| `por_producto` | arreglo | sí | todos los productos activos, incluso en 0 |
| `existencia_inicial` | entero | sí | **0 en v1** (cada día es independiente) |
| `producidas` | entero | sí | suma de hornadas del día |
| `vendidas` | entero | sí | suma de ventas con `fecha` = día |
| `descartadas` | entero | sí | declaradas por el encargado |
| `diferencia` | entero | sí | `producidas − vendidas − descartadas` |
| `existencia_final` | entero | sí | `diferencia`; no se arrastra al día siguiente |
| `lotes_finales` | arreglo | sí | `[]` en v1; reservado para P2 |
| `costo_aplicado_centavos` | entero | sí | costo congelado al cerrar |
| `merma_centavos` | entero | sí | `descartadas × costo_aplicado_centavos` |
| `merma_bp` | entero | sí | `descartadas × 10000 ÷ producidas`, redondeo al entero |
| `total_merma_centavos` | entero | sí | suma de `merma_centavos` |

Reglas:

- La diferencia **positiva** exige `confirmo_diferencia: true`; si falta, `422`
  con `confirmacion_requerida`.
- La diferencia **negativa** se rechaza con `422` y `diferencia_negativa`; el
  cierre no se guarda.
- El cierre es inmutable: no se edita ni se borra. Reenviar el mismo contenido
  devuelve `200` con `ya_procesada: true`; otro contenido devuelve `409` con
  `cierre_existente`.
- El costo se congela en `costo_aplicado_centavos` para que un cambio posterior
  de costos no altere cierres anteriores.
- Un cajero no puede cerrar (`403`).

> **Compatibilidad con el ODT:** el ODT muestra `existencia_inicial: 20` y
> `lotes_finales` con unidades. En v1 el checklist 3.3 fija existencia inicial
> 0 y no hay arrastre entre días; por eso `existencia_inicial` es 0 y
> `lotes_finales` es `[]`. Los campos se mantienen para no romper el formato y
> quedan listos para la extensión P2.

### 8.1 Ejemplo inválido

Producidas 150, vendidas 140, descartadas 20 → `diferencia = −10`:

```json
{
  "fecha": "2026-10-13",
  "descartadas": [{ "producto_id": "producto:pan-frances", "unidades": 20 }],
  "confirmo_diferencia": false
}
```
→ `422` con `codigo: "diferencia_negativa"`.

---

## 9. `usuario` — base `usuarios`

Nunca se replica al cliente. La contraseña se guarda **solo** como hash
`argon2`, nunca en claro ni en logs.

```json
{
  "_id": "usuario:cajero1",
  "_rev": "1-4e5f...",
  "tipo": "usuario",
  "nombre": "cajero1",
  "rol": "cajero",
  "hash_argon2": "$argon2id$v=19$m=19456,t=2,p=1$...",
  "activo": true,
  "creado_en": "2026-10-13T07:00:00-06:00"
}
```

| Campo | Tipo | Obligatorio | Regla |
|---|---|---|---|
| `_id` | string | sí | `usuario:{nombre}` |
| `tipo` | string | sí | `usuario` |
| `nombre` | string | sí | `[a-z0-9-]`, 1–64 |
| `rol` | string | sí | `cajero` o `encargado` |
| `hash_argon2` | string | sí | hash argon2id; nunca la contraseña |
| `activo` | booleano | sí | si es `false`, no puede iniciar sesión |
| `creado_en` | string | sí | RFC 3339 con zona |

Permisos:

| Acción | `cajero` | `encargado` |
|---|---|---|
| `POST /ventas` | sí | sí |
| `POST /hornadas` | no (`403`) | sí |
| `POST /cierre` | no (`403`) | sí |
| `GET /reportes/ventas` | no (`403`) | sí |
| `GET /merma` | no (`403`) | sí |

### 9.1 Ejemplo inválido

```json
{ "_id": "usuario:ana", "tipo": "usuario", "nombre": "ana", "rol": "admin", "hash_argon2": "...", "activo": true, "creado_en": "2026-10-13T07:00:00-06:00" }
```
→ `422`: `rol` no es `cajero` ni `encargado`.

---

## 10. `vencimientos` — reporte derivado (no se guarda)

`GET /vencimientos` calcula este objeto a partir de las hornadas y de
`vida_util_dias`. No es un documento de CouchDB.

```json
{
  "fecha": "2026-10-13",
  "actualizado_en": "2026-10-13T15:02:11-06:00",
  "resumen": { "vencido": 0, "vence_hoy": 0, "por_vencer": 42, "vigente": 0 },
  "lotes": [
    {
      "lote_id": "hornada:01J9ZK5A2B3C4D5E6F7G8H9J0K",
      "producto_id": "producto:pan-frances",
      "unidades": 42,
      "fecha_vencimiento": "2026-10-14",
      "dias_restantes": 1,
      "estado": "por_vencer"
    }
  ]
}
```

Estados según `dias_restantes = fecha_vencimiento − fecha`:

| `dias_restantes` | `estado` |
|---|---|
| `< 0` | `vencido` |
| `0` | `vence_hoy` |
| `1` | `por_vencer` |
| `> 1` | `vigente` |

---

## 11. Resumen de errores

Forma única:

```json
{
  "codigo": "validacion",
  "errores": [
    { "campo": "lineas[0].cantidad", "motivo": "debe estar entre 1 y 1000" }
  ]
}
```

| `codigo` | HTTP | Cuándo |
|---|---|---|
| `solicitud_invalida` | 422 | JSON mal formado |
| `validacion` | 422 | campos inválidos |
| `diferencia_negativa` | 422 | cierre con diferencia < 0 |
| `confirmacion_requerida` | 422 | diferencia > 0 sin confirmar |
| `rango_invalido` | 422 | fechas de reporte inválidas o > 31 días |
| `credenciales_invalidas` | 401 | login incorrecto |
| `no_autorizado` | 401 | falta token o expiró |
| `prohibido` | 403 | rol insuficiente |
| `operacion_en_conflicto` | 409 | mismo ULID, otra huella |
| `cierre_existente` | 409 | día ya cerrado con otro contenido |
| `no_encontrado` | 404 | producto/config inexistente |
| `no_disponible` | 503 | CouchDB no responde |
| `dependencia_no_disponible` | 503 | `/salud` sin CouchDB |

---

## 12. Versionado

- Este contrato se congela con el tag `contrato-v1` (ítem 3.6) y se copia en
  `panaderia-frontend/contratos/`.
- Cualquier cambio posterior requiere issue y PR en ambos repos, y una nueva
  versión (`contrato-v2`).
- Los cambios incompatibles incluyen: renombrar campos, cambiar tipos, cambiar
  la fórmula de la huella, cambiar la política de `_id` o la semántica de
  `fecha`/`fecha_original`.
