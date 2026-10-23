# Aspectos éticos

- **Fecha:** 2026-10-22
- **Estado:** propuesta para el protocolo del proyecto
- **Relacionado:** ítems 3.11, 4.2, 4.3, 4.28 y 6.1

## 1. Principio general

El proyecto no trabaja con personas ni con datos personales reales. Toda la
información de ventas, producción, descartes y usuarios es **sintética** y se
genera con una semilla fija para que el experimento sea reproducible. Aun así,
se aplican controles de acceso, minimización y manejo seguro de credenciales,
porque el sistema podría desplegarse con datos reales en el futuro.

## 2. Datos sintéticos

- Los datos se generan con `datos-sinteticos/` y una **semilla fija**; no se
  copian de una panadería real ni de bases de clientes.
- Los nombres de usuario son genéricos (`cajero1`, `cajero2`, `encargado1`), sin
  nombres, correos, teléfonos, direcciones ni documentos de identidad.
- Los `dispositivo_id` son etiquetas (`caja-1`, `caja-2`), no identificadores de
  hardware.
- Las fechas y montos son plausibles pero inventados; no corresponden a ventas
  reales.
- Los datos sintéticos se publican con el repositorio para permitir la
  reproducción, siempre que no contengan secretos.

## 3. Anonimización y minimización

Aunque no haya datos personales, se aplican estas reglas:

- No se recogen campos que no sean necesarios para la venta, la producción, el
  cierre o el reporte.
- No se guardan IPs, user agents ni geolocalización en los documentos del
  negocio.
- Si un log de infraestructura incluye una IP, se trunca o se hash antes de
  publicarlo.
- Los datos de prueba que se compartan con el equipo no incluyen tokens,
  contraseñas ni hashes de contraseñas.
- La unidad de análisis del experimento es la repetición; no se perfilan
  personas ni cajas individuales.

## 4. Control de acceso

- CouchDB tiene tres bases con permisos distintos:
  - `catalogo`: `cliente` solo lee; `backend` escribe.
  - `panaderia`: solo `backend`.
  - `usuarios`: solo `backend`.
- La API aplica roles: `cajero` no registra hornadas ni cierra el día; `encargado`
  sí.
- El frontend no recibe costos, hashes de contraseñas ni tokens de otros
  usuarios.
- La réplica de la PWA es de **solo bajada** para el catálogo; la cola de
  pendientes no se replica.
- El acceso directo a la base oficial con las credenciales de `cliente` debe
  fallar para `panaderia` y `usuarios` (prueba del ítem 3.8 y 6.6).

## 5. Credenciales y secretos

- Las contraseñas de CouchDB, PostgreSQL, JWT y usuarios de aplicación viven en
  `.env`, que **no se versiona**.
- `.env.example` solo contiene valores de ejemplo, nunca reales.
- No se guardan contraseñas en claro: los usuarios de la aplicación usan hash
  `argon2`; CouchDB maneja su propio hash.
- No se imprimen contraseñas, tokens ni hashes completos en logs, capturas,
  issues o PR.
- Los scripts de siembra leen las contraseñas del entorno; no las escriben en
  Git.
- Si una credencial se filtra, se rota y se documenta el incidente; no basta con
  borrar el commit.
- El repositorio no debe contener hashes de administrador en `local.ini`; el
  administrador se crea desde las variables de entorno al inicializar el
  volumen.

## 6. Ciclo de vida de los datos

| Etapa | Regla |
|---|---|
| Generación | semilla fija, datos sintéticos, sin PII |
| Uso | solo para pruebas, experimento y demostración |
| Almacenamiento | volúmenes locales de Docker; no se suben a servicios externos |
| Publicación | datos sintéticos y resultados agregados; sin secretos |
| Retención | mientras dure el proyecto; se pueden borrar los volúmenes sin afectar a personas |
| Eliminación | `docker compose down -v` y borrado de `resultados/` si contienen datos sensibles |

## 7. Riesgos y mitigaciones

| Riesgo | Probabilidad | Impacto | Mitigación |
|---|---|---|---|
| Filtración de una contraseña en Git | media | alto | `.env` ignorado, `.env.example` sin secretos, revisión de PR, rotación |
| Reidentificación a partir de dispositivos y horarios | baja | medio | IDs genéricos, datos sintéticos, no publicar microdatos con nombres |
| Uso de datos reales por error | baja | alto | el repositorio solo contiene datos sintéticos; el README lo advierte |
| Acceso indebido a `panaderia` o `usuarios` | media | alto | `_security`, `validate_doc_update`, prueba de permisos |
| Tokens en logs o en documentos | media | alto | no registrar tokens; token en almacenamiento del navegador, no en PouchDB |
| Sesgo del experimento por una sola arquitectura | media | medio | mismo hardware, datos, límites y orden aleatorio |

## 8. Uso académico y transparencia

- Se citan las fuentes, librerías y herramientas usadas (APA 7).
- Los resultados se reportan completos, incluidos los no esperados y los
  negativos.
- No se atribuye una diferencia solo al lenguaje: se describen las diferencias
  de arquitectura y sus limitaciones.
- Los scripts, la semilla y los metadatos permiten reproducir el experimento.
- El informe declara que los datos son sintéticos y que no se trabajó con seres
  humanos, por lo que no se requiere consentimiento informado.

## 9. Responsabilidades

| Rol | Responsabilidad |
|---|---|
| Coordinador | mantener este documento y el `.env.example` |
| Responsable de infraestructura | credenciales, permisos de CouchDB y respaldos |
| Responsable de datos | generación sintética y ausencia de PII |
| Revisor de PR | detectar secretos y datos personales antes de fusionar |
| Todo el equipo | no subir `.env`, tokens, contraseñas ni datos reales |

## 10. Criterios de aceptación

1. `git grep` no encuentra contraseñas, tokens ni hashes reales.
2. `.env` está ignorado y `.env.example` no tiene secretos.
3. Los datos de `datos-sinteticos/` no contienen PII.
4. `cliente` no puede leer `panaderia` ni `usuarios` (prueba automatizada).
5. `cliente` no puede escribir `catalogo`.
6. Los logs de la API no imprimen tokens ni contraseñas.
7. El informe declara el carácter sintético de los datos y las limitaciones.
