# Hipótesis del experimento

- **Fecha:** 2026-10-22
- **Estado:** propuesta, se confirma con el piloto (5.1) antes de ejecutar
- **Relacionado:** `docs/experimento.md`, ítems 3.9, 3.10, 5.2 y 5.6

## 1. Pregunta

¿La arquitectura **Rust + Axum + CouchDB** ofrece mejor latencia y capacidad de
procesamiento que la línea base **Node.js + Express + PostgreSQL** al registrar
ventas y consultar reportes, con los mismos datos, reglas, hardware y
condiciones de red?

No se pregunta si Rust es “mejor” en abstracto: se comparan **arquitecturas
completas**. Una diferencia puede deberse al lenguaje, al driver de base de
datos, al modelo de concurrencia o a la configuración, y el informe debe decirlo
así (ver `docs/memoria/limitaciones.md`).

## 2. Unidad de análisis

La unidad es la **repetición**, no cada petición. Cada repetición es una
ejecución completa de un escenario contra una arquitectura, con su propio
conjunto de peticiones y su propio resumen (p50, p95, p99, throughput, errores,
CPU y RAM). Analizar peticiones individuales inflaría el tamaño de muestra y
violaría la independencia.

## 3. Hipótesis principales

Sean `L_R` y `L_N` la mediana (o el p95, según se declare antes del piloto) de
la latencia por repetición de `POST /ventas` en Rust y Node, y `T_R`, `T_N` el
throughput por repetición.

### H0 (nula)

- **H0_latencia:** no hay diferencia en la latencia típica de `POST /ventas`
  entre las dos arquitecturas: `L_R = L_N`.
- **H0_throughput:** no hay diferencia en el throughput de `POST /ventas`:
  `T_R = T_N`.

### H1 (alternativa)

- **H1_latencia:** la arquitectura Rust + CouchDB tiene **menor** latencia
  típica en `POST /ventas` que la línea base: `L_R < L_N`.
- **H1_throughput:** la arquitectura Rust + CouchDB tiene **mayor** throughput
  en `POST /ventas` que la línea base: `T_R > T_N`.

Las mismas hipótesis se plantean, como **secundarias**, para
`GET /reportes/ventas` y para los tres escenarios de red. Se declaran antes de
ejecutar; no se cambian después de ver los datos.

## 4. Métricas

| Métrica | Definición | Unidad |
|---|---|---|
| Latencia p50/p95/p99 | percentiles de los tiempos de respuesta **por repetición** | ms |
| Throughput | peticiones completadas ÷ duración efectiva de la repetición | req/s |
| Tasa de error | respuestas no exitosas ÷ peticiones | % |
| CPU | uso promedio y pico de la API y de la base | % / núcleos |
| RAM | uso promedio y pico de la API y de la base | MiB |
| Cola de sincronización | tiempo para vaciar N ventas pendientes tras reconectar | s |
| Tasa de aceptación tras reconectar | ventas aceptadas ÷ ventas pendientes | % |

La métrica primaria es la **latencia p95 de `POST /ventas` por repetición**. Las
demás son secundarias y se reportan con intervalos de confianza.

## 5. Variables

### Independiente

- Arquitectura: `rust-couchdb` o `node-postgres`.
- Escenario de red: estable, pérdida de paquetes, ancho de banda limitado.
- Número de repeticiones `N` (se fija con el piloto).

### Dependientes

- Latencia, throughput, errores, CPU, RAM y métricas de sincronización.

### Controladas

- Mismo hardware y mismos límites de CPU/RAM para ambos tratamientos.
- Mismos datos sintéticos (semilla fija, mismo catálogo y mismas ventas).
- Mismo contrato, mismos payloads, mismo número de clientes y misma duración.
- Mismo sistema operativo, versiones de contenedores y red aislada.
- Calentamiento (warm-up) idéntico antes de medir.
- Orden de ejecución aleatorizado por bloques.

### Extrañas o de confusión

- Caché del sistema operativo y de la base.
- Compilación en modo release vs debug.
- Diferencias de driver (Reqwest vs pg) y de modelo de datos.
- Ruido de la máquina anfitriona.
- Hora del día y temperatura del equipo.

Se mitigan con aleatorización, repeticiones, mismo hardware, límites de
contenedores, warm-up y registro de metadatos del entorno.

## 6. Regla de decisión

- Prueba principal: **Mann-Whitney U bilateral** sobre los valores por
  repetición (no asume normalidad), con `α = 0.05`.
- Tamaño del efecto: **delta de Cliff** y diferencia de medianas con intervalo
  de confianza por bootstrap (10 000 remuestreos).
- Si `p < 0.05` y el intervalo de confianza de la diferencia no incluye 0, se
  rechaza H0 para esa métrica.
- La significancia estadística **no basta**: se exige una diferencia práctica
  mínima acordada en el piloto (por ejemplo, 10 % en p95 o 1 ms, lo que resulte
  más exigente) para afirmar una mejora relevante.
- Si no se rechaza H0, se reporta como “no se encontró evidencia de diferencia”,
  no como “son iguales”.
- Los resultados no esperados se reportan igual (5.6).

## 7. Tamaño de muestra

`N` no se fija por conveniencia: sale del piloto (5.1), de la variabilidad
observada, del efecto mínimo de interés y de la potencia deseada (0.8). El
objetivo de trabajo es `N = 30` repeticiones por celda
(3 escenarios × 2 arquitecturas), pero el piloto puede ajustarlo. Si `N` cambia,
se documenta antes de la corrida definitiva.

## 8. Qué falsaría H1

- Que la latencia de Rust sea igual o mayor que la de Node en la métrica
  primaria, con diferencia práctica y significativa.
- Que el throughput de Rust sea igual o menor.
- Que la ventaja desaparezca al controlar el escenario de red o al usar los
  mismos límites de recursos.
- Que la diferencia se explique por un fallo metodológico (por ejemplo, Node
  midiendo con menos datos o sin índices), en cuyo caso se corrige y se repite.

## 9. Relación con el resto del proyecto

- El diseño operativo (endpoints, repeticiones, aleatorización, `tc`, límites)
  está en `docs/experimento.md`.
- La ética y el manejo de datos están en `docs/memoria/etica.md`.
- Los resultados y el contraste formal se escriben en
  `docs/memoria/resultados.md` (5.6).
- Las metas SMART se justifican con el piloto en `docs/experimento.md` (5.2).
