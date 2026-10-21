# ADR: Cierre diario y cálculo de la merma

- **Fecha:** 2026-10-21
- **Estado:** Propuesta (se confirma al congelar `contrato-v1`, ítem 3.6)
- **Responsable:** coordinador
- **Relacionado:** `contratos/documentos.md` (documento `cierre`), ítem 3.4

## Contexto

Para medir la merma hay que comparar lo que se produjo con lo que se vendió y se descartó. Las ventas y las hornadas se guardan como eventos y el cierre resume el día. Hace falta una fórmula única y reglas claras para que Rust y la línea base calculen lo mismo y para que un cierre no cambie después de guardado.

## Decisión

1. El cierre lo hace el **encargado**, **con conexión**, una vez por día de negocio.
2. **Cada día es independiente:** la existencia inicial es 0. Lo producido y no vendido es descarte o diferencia.
3. Fórmula por producto: `diferencia = producidas − vendidas − descartadas`.
   - `producidas`: suma de las unidades de las hornadas del día. La calcula Rust.
   - `vendidas`: suma de las cantidades de las ventas con esa fecha. La calcula Rust.
   - `descartadas`: las declara el encargado.
4. **Merma = unidades descartadas**, valoradas a **costo**, no a precio de venta: `merma_centavos = descartadas × costo_aplicado_centavos`.
   - `merma_bp = descartadas × 10000 ÷ producidas`, redondeado al entero más cercano (mitades hacia arriba); 0 si `producidas` es 0.
5. **Diferencia positiva:** unidades sin explicar. Se muestra aparte, **no cuenta como merma** y exige confirmación explícita (`confirmo_diferencia: true`). Si falta, `422` con `confirmacion_requerida`.
6. **Diferencia negativa:** es una inconsistencia (por ejemplo, falta registrar una hornada). Se rechaza con `422` y `diferencia_negativa`, y el cierre no se guarda.
7. **Costo aplicado:** el cierre guarda el costo vigente de `config:vigente` (`costo_aplicado_centavos`), para que un cambio de costos no altere los cierres anteriores.
8. **Inmutable:** el `_id` es `cierre:{fecha}` y el cierre no se edita ni se borra.
   - Reenviar el mismo contenido (misma `huella`) devuelve `200` con `ya_procesada: true`.
   - Otro contenido para un día ya cerrado devuelve `409` con `cierre_existente`.
9. El cierre incluye **todos los productos activos**, aunque estén en 0.
10. Un usuario con rol `cajero` no puede cerrar (`403`).

## Ejemplo (pan francés)

Producidas 150, vendidas 128, descartadas 20, costo 5 centavos:

- `diferencia = 150 − 128 − 20 = 2`. Es positiva: se muestra aparte y requiere confirmación.
- `merma_centavos = 20 × 5 = 100` (un dólar).
- `merma_bp = 20 × 10000 ÷ 150 = 1333` (13.33 %).

Caso rechazado: producidas 150, vendidas 140, descartadas 20 da `diferencia = −10`. El cierre se rechaza con `diferencia_negativa`.

## Alternativas descartadas

| Alternativa | Motivo |
|---|---|
| Existencias que pasan de un día al siguiente (lotes) | Agrega mucha lógica (orden de consumo, cierres encadenados) y no mejora las métricas del experimento. Queda como extensión P2 |
| Merma = producidas − vendidas, calculada sola | Mezcla lo que de verdad se tiró con errores de registro y unidades sin explicar |
| Valorar la merma al precio de venta | Mide ingreso no percibido, no desperdicio. El costo es lo que cuesta producir lo que se tiró |
| Permitir editar o anular un cierre | Rompe la trazabilidad e invalida los reportes |
| Bloquear el cierre ante cualquier diferencia positiva | Impediría cerrar por faltantes pequeños. La confirmación explícita lo registra sin bloquear |

## Consecuencias

- Rust y la línea base usan la misma fórmula y se prueban con los ejemplos de este documento.
- El encargado debe declarar los descartes cada día. Si no lo hace, todo lo no vendido aparece como diferencia y no como merma.
- Un error de registro de una hornada se descubre al cierre (diferencia negativa) y debe corregirse antes de poder cerrar.
- Un cierre equivocado **no se puede corregir** en esta versión. Es una limitación aceptada.
- Si más adelante se adoptan lotes, la fórmula pasa a `existencia_inicial + producidas − vendidas − descartadas − existencia_final`, y el contrato se versiona.

## Ventas de un día ya cerrado

(Se completa en el ítem 3.4.)