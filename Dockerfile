# Imagen de la API. Compila en la imagen para que el servidor no tenga que
# compilar al arrancar el contenedor.
FROM rust:latest

WORKDIR /app

# Copiar el proyecto y compilar la versión de producción.
COPY . .
RUN cargo build --release --locked

ENV PORT=3000
EXPOSE 3000

# Ejecuta el binario ya compilado.
CMD ["./target/release/panaderia-backend"]
