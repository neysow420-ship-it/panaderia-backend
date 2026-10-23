// Validación de escrituras de la base `catalogo`.
//
// - Solo el usuario con rol `backend` (o un administrador del servidor) escribe.
// - El usuario `cliente` puede leer, pero no crear, modificar ni borrar.
// - Cada documento debe ser un `producto:{slug}` con los campos del contrato.
function (newDoc, oldDoc, userCtx, secObj) {
  var roles = userCtx.roles || [];
  var esBackend =
    roles.indexOf("backend") !== -1 || roles.indexOf("_admin") !== -1;

  if (!esBackend) {
    throw({ forbidden: "solo el usuario backend puede escribir en catalogo" });
  }

  // El backend sí puede administrar los design docs (vistas, validación).
  if (newDoc._id && newDoc._id.indexOf("_design/") === 0) {
    return;
  }

  if (newDoc._deleted) {
    return;
  }

  if (newDoc.tipo !== "producto") {
    throw({ forbidden: "solo se permiten documentos tipo producto" });
  }
  if (
    typeof newDoc._id !== "string" ||
    newDoc._id.indexOf("producto:") !== 0
  ) {
    throw({ forbidden: "el _id debe tener la forma producto:<slug>" });
  }
  if (
    typeof newDoc.nombre !== "string" ||
    newDoc.nombre.trim().length === 0
  ) {
    throw({ forbidden: "nombre es obligatorio" });
  }
  if (
    typeof newDoc.precio_centavos !== "number" ||
    newDoc.precio_centavos < 0 ||
    Math.floor(newDoc.precio_centavos) !== newDoc.precio_centavos
  ) {
    throw({ forbidden: "precio_centavos debe ser un entero no negativo" });
  }
  if (
    typeof newDoc.vida_util_dias !== "number" ||
    newDoc.vida_util_dias < 1 ||
    newDoc.vida_util_dias > 365 ||
    Math.floor(newDoc.vida_util_dias) !== newDoc.vida_util_dias
  ) {
    throw({ forbidden: "vida_util_dias debe ser un entero entre 1 y 365" });
  }
  if (typeof newDoc.activo !== "boolean") {
    throw({ forbidden: "activo debe ser booleano" });
  }
}
