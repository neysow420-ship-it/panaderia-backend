# Diseño experimental

- **Fecha:** 2026-10-22
- **Estado:** propuesta; se cierra con el piloto (5.1) y la firma de Estadística
- **Relacionado:** `docs/memoria/hipotesis.md`, ítems 3.10, 4.30–4.38 y 5.1–5.5

## 1. Objetivo

Comparar el rendimiento de dos arquitecturas completas con el mismo contrato,
datos y carga:

| Tratamiento | API | Base de datos |
|---|---|---|
| `rust-couchdb` | Rust + Axum + Tokio | CouchDB 3.3 |
| `node-postgres` | Node.js + Express | PostgreSQL 16 |

La comparación es de **arquitecturas**, no solo de lenguajes. El informe debe
distinguir lo atribuible al lenguaje de lo atribuible al motor de base de datos,
al driver o a la configuración.

## 2. Endpoints comparados

| Endpoint | Uso en el experimento | Datos |
|---|---|---|
| `POST /ventas` | escritura concurrente en hora pico | ventas sintéticas con ULID y líneas del catálogo |
| `GET /reportes/ventas?desde&hasta` | lectura agregada de 7 días | mismas ventas sembradas en ambas arquitecturas |

No se comparan `/salud`, `/auth/login` ni `/merma`: no son las operaciones que
mide el objetivo. Si se miden como diagnóstico, se reportan aparte y no entran
en la prueba principal.

## 3. Repetición

Una **repetición** es una ejecución independiente y completa de un escenario
contra una arquitectura:

1. Se restaura la base a un estado conocido (snapshot o siembra con semilla
   fija).
2. Se levantan los contenedores con los límites de recursos.
3. Se hace un calentamiento de 10 s (no se mide).
4. Se ejecuta la carga durante 60 s (se mide).
5. Se espera 10 s de enfriamiento y se recogen métricas y metadatos.
6. Se apagan los contenedores y se guarda el resultado.

Cada repetición produce **un** valor por métrica (p50, p95, p99, throughput,
errores, CPU, RAM). No se analizan peticiones individuales como unidades
independientes.

## 4. Carga

- **Escritura:** `POST /ventas` con una tasa objetivo de 20 req/s durante 60 s
  (1 200 ventas por repetición), 20 clientes virtuales y ULID únicos generados
  con semilla fija.
- **Lectura:** `GET /reportes/ventas` con un rango fijo de 7 días, 1 req/s
  durante 60 s, después de sembrar al menos 3 semanas de datos.
- **Mezcla:** si el tiempo alcanza, una tercera carga mixta 80 % ventas / 20 %
  reportes; se reporta como secundaria.
- El mismo script de k6, los mismos payloads y la misma semilla se usan contra
  ambas arquitecturas. El token se obtiene una vez en `setup()` si el login está
  activo.

Los valores exactos (tasa, VUs, duración) se confirman en el piloto y quedan
guardados con los resultados.

## 5. Escenarios de red

Se ejecutan tres escenarios con `tc` en Linux. El `tc` se aplica al contenedor
que genera la carga, para que el camino de red sea equivalente para las dos
arquitecturas.

| Escenario | Parámetros `tc` | Objetivo |
|---|---|---|
| `red-estable` | sin `netem` | referencia |
| `perdida-5` | `netem loss 5%` | reintentos y robustez |
| `banda-latencia` | `tbf rate 2mbit burst 32kbit latency 400ms` + `netem delay 100ms` | ancho de banda y latencia típicos de una conexión móvil mala |

Comandos de referencia (se guardan en `experimentos/red/`):

```bash
# Pérdida
tc qdisc add dev eth0 root netem loss 5%

# Banda y latencia
tc qdisc add dev eth0 root handle 1: tbf rate 2mbit burst 32kbit latency 400ms
tc qdisc add dev eth0 parent 1:1 handle 10: netem delay 100ms

# Limpiar
tc qdisc del dev eth0 root
```

Antes de ejecutar se comprueba que no hay otro tráfico en la interfaz y que el
reloj de los contenedores está sincronizado.

## 6. Aleatorización y orden

- Se agrupan las repeticiones en **bloques de 2**: dentro de cada bloque se
  ejecutan las dos arquitecturas en orden aleatorio.
- La semilla de aleatorización se fija antes de empezar y se guarda en los
  metadatos.
- Los escenarios también se aleatorizan entre bloques para evitar que el
  calentamiento de la máquina favorezca siempre al mismo tratamiento.
- La asignación se registra en un archivo (`orden.csv`) antes de la primera
  repetición; no se cambia después de ver resultados.

## 7. Hardware y límites

- Ambas arquitecturas corren en **el mismo equipo físico**, el mismo día y con
  el mismo sistema operativo y versiones de Docker.
- Límites por contenedor (valores iniciales; el piloto puede ajustarlos y los
  definitivos se congelan en `resultados/VERSIONES.md`):

| Componente | CPU | RAM |
|---|---|---|
| API Rust | 1.0 | 512 MiB |
| API Node | 1.0 | 512 MiB |
| CouchDB | 1.0 | 1 GiB |
| PostgreSQL | 1.0 | 1 GiB |
| Generador k6 | 2.0 | 1 GiB |

- Se registran CPU, RAM, sistema operativo, versiones, límites y parámetros de
  red en cada resultado.
- No se ejecutan otras cargas en la máquina durante la medición.

## 8. Métricas por repetición

| Métrica | Cómo se obtiene |
|---|---|
| Latencia p50/p95/p99 | resumen de k6 por endpoint y repetición |
| Throughput | peticiones completadas ÷ segundos efectivos |
| Tasa de error | respuestas no 2xx ÷ total |
| CPU/RAM API | muestreo de `docker stats` cada segundo; promedio y pico |
| CPU/RAM base | idem |
| Tiempo de vaciado de cola | cliente de simulación: desde reconectar hasta aceptar la última venta |
| Tasa de aceptación | aceptadas ÷ pendientes tras reconectar |

La telemetría de Rust (4.31) y el cliente de simulación (4.35) deben medir lo
mismo en ambas arquitecturas. Se documenta el punto exacto de inicio y fin del
cronómetro para no medir trabajo distinto.

## 9. Prueba estadística

Antes de ejecutar, Estadística revisa y firma:

- **Prueba principal:** Mann-Whitney U bilateral por métrica, con `α = 0.05`.
- **Tamaño del efecto:** delta de Cliff y diferencia de medianas.
- **Intervalo de confianza:** bootstrap de 10 000 remuestreos sobre la
  diferencia de medianas.
- **Potencia:** 0.8 para el efecto mínimo de interés.
- **Múltiples métricas:** se declara la primaria (`p95` de `POST /ventas`) y
  las secundarias se reportan con corrección de Holm o se marcan como
  exploratorias.
- **Supuestos:** independencia entre repeticiones, misma distribución dentro de
  cada tratamiento. Si hay dependencia por bloque, se usa un modelo mixto o se
  justifica la alternativa.
- **Datos faltantes o fallos:** se reportan; no se eliminan repeticiones sin
  documentar la causa.

| Revisión de Estadística | Nombre | Fecha | Firma |
|---|---|---|---|
| Diseño y prueba acordados |  |  |  |

## 10. Recolección y almacenamiento

- Datos crudos sin editar en `resultados/{fecha}/{escenario}/{arquitectura}/`.
- Cada repetición guarda: resumen de k6, CSV de telemetría, `docker stats`,
  logs de la API, versión de commit, hashes, semilla, parámetros de red,
  hardware y límites.
- `experimentos/run-experimento.sh` registra comando, semilla, parámetros,
  versión y hashes, y copia los metadatos del entorno (4.38).
- No se sobrescriben resultados: una repetición nueva es una carpeta nueva.

## 11. Procedimiento

1. Congelar código y contrato (`exp-01`).
2. Sembrar datos sintéticos con semilla fija y verificar conteos en ambas bases.
3. Ejecutar el piloto (5.1) con pocas repeticiones y ajustar metas, duración y
   `N`.
4. Firmar el diseño con Estadística.
5. Generar el orden aleatorio y guardarlo.
6. Ejecutar 3 escenarios × 2 arquitecturas × `N` repeticiones.
7. Verificar que no falten resultados y que los metadatos estén completos.
8. Analizar en el notebook (5.5) y contrastar H0/H1 (5.6).

## 12. Amenazas a la validez

| Amenaza | Mitigación |
|---|---|
| Una arquitectura recibe más datos | Misma siembra, mismos conteos y mismo script |
| Diferencias de configuración | Mismos límites, mismas versiones, mismo hardware |
| Caché favorable a la primera corrida | Warm-up y orden aleatorio |
| Reloj o red distintos | Mismo host, misma red, `tc` en el generador |
| Medir peticiones, no repeticiones | Resumen por repetición; la repetición es la unidad |
| Cambiar la prueba después de ver datos | Diseño firmado antes de ejecutar |
| Atribuir todo al lenguaje | Reportar arquitectura completa y limitaciones |

## 13. Relación con el checklist

| Ítem | Dónde se cumple |
|---|---|
| 3.10 | este documento |
| 4.30 | `infra/docker-compose.yml` completo con límites y healthchecks |
| 4.31 | `src/telemetria/` |
| 4.32 | `experimentos/recursos/` |
| 4.33 | `experimentos/k6/` |
| 4.34 | `experimentos/red/` |
| 4.35 | `experimentos/sync/` |
| 4.38 | `experimentos/run-experimento.sh` y `resultados/` |
| 5.1–5.6 | piloto, metas, corrida, notebook y contraste |
