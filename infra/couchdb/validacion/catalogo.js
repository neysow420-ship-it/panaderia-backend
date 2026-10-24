// Validación de escrituras de la base `catalogo`.
//
// - Solo `backend` o un administrador puede escribir.
// - `cliente` puede leer, pero no escribir.
// - Solo se permiten documentos de productos válidos.
// - Los costos internos nunca pueden guardarse en el catálogo.

(function (newDoc, oldDoc, userCtx, secObj) {
  var roles = userCtx.roles || [];

  var esBackend =
    roles.indexOf("backend") !== -1 ||
    roles.indexOf("_admin") !== -1;

  // Solo backend o un administrador puede escribir.
  if (!esBackend) {
    throw({
      forbidden: "solo el usuario backend puede escribir en catalogo"
    });
  }

  // Permitir al backend administrar vistas y validaciones.
  if (
    typeof newDoc._id === "string" &&
    newDoc._id.indexOf("_design/") === 0
  ) {
    return;
  }

  // Permitir al backend eliminar documentos.
  if (newDoc._deleted) {
    return;
  }

  // Los costos son exclusivamente internos.
  for (var campo in newDoc) {
    if (
      Object.prototype.hasOwnProperty.call(newDoc, campo) &&
      /costo/i.test(campo)
    ) {
      throw({
        forbidden: "los costos internos no pueden guardarse en catalogo"
      });
    }
  }

  // Solo se permiten documentos de productos.
  if (newDoc.tipo !== "producto") {
    throw({
      forbidden: "solo se permiten documentos tipo producto"
    });
  }

  // Validar el identificador.
  if (
    typeof newDoc._id !== "string" ||
    newDoc._id.indexOf("producto:") !== 0
  ) {
    throw({
      forbidden: "el _id debe tener la forma producto:<slug>"
    });
  }

  var slug = newDoc._id.substring("producto:".length);

  if (!/^[a-z0-9-]{1,64}$/.test(slug)) {
    throw({
      forbidden: "el slug debe usar minúsculas, números y guiones"
    });
  }

  // Validar el nombre.
  if (
    typeof newDoc.nombre !== "string" ||
    newDoc.nombre.trim().length === 0
  ) {
    throw({
      forbidden: "nombre es obligatorio"
    });
  }

  // El precio debe ser un entero no negativo en centavos.
  if (
    typeof newDoc.precio_centavos !== "number" ||
    !isFinite(newDoc.precio_centavos) ||
    newDoc.precio_centavos < 0 ||
    Math.floor(newDoc.precio_centavos) !== newDoc.precio_centavos
  ) {
    throw({
      forbidden: "precio_centavos debe ser un entero no negativo"
    });
  }

  // La vida útil debe ser un entero entre 1 y 365 días.
  if (
    typeof newDoc.vida_util_dias !== "number" ||
    !isFinite(newDoc.vida_util_dias) ||
    newDoc.vida_util_dias < 1 ||
    newDoc.vida_util_dias > 365 ||
    Math.floor(newDoc.vida_util_dias) !== newDoc.vida_util_dias
  ) {
    throw({
      forbidden: "vida_util_dias debe ser un entero entre 1 y 365"
    });
  }

  // El estado del producto debe ser booleano.
  if (typeof newDoc.activo !== "boolean") {
    throw({
      forbidden: "activo debe ser booleano"
    });
  }
})