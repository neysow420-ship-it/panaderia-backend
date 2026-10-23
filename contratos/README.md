# Contratos

| Archivo | Qué es |
|---|---|
| `documentos.md` | Contrato canónico de los documentos de CouchDB |
| `openapi.yaml` | Contrato canónico de la API (7 endpoints) |

## Congelación `contrato-v1` (ítem 3.6)

Aún **no** está congelado. Pasos para hacerlo:

1. Revisión por pares de `documentos.md` y `openapi.yaml` por alguien distinto
   del autor (issue + PR).
2. Verificar que el contrato coincide con el código y con la línea base:
   - `POST /ventas` devuelve `{ ya_procesada, venta }`;
   - los errores usan `{ codigo, errores: [{ campo, motivo }] }`;
   - las fechas de negocio usan `America/El_Salvador`;
   - el dinero va en centavos enteros.
3. Crear el tag en este repo:

   ```bash
   git tag -a contrato-v1 -m "Contrato de documentos y API v1"
   git push origin contrato-v1
   ```

4. Copiar `contratos/documentos.md` y `contratos/openapi.yaml` a
   `panaderia-frontend/contratos/` y crear el mismo tag en el repo del
   frontend.
5. A partir de ahí, cualquier cambio requiere issue y PR en ambos repos y una
   nueva versión (`contrato-v2`).

El CI del frontend debe comprobar que su copia coincide con el tag
`contrato-v1` del backend (recomendación R.19).
