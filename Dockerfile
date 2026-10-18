# Imagen oficial de Rust
FROM rust:1.73

WORKDIR /app

# Copiar los archivos del proyecto al contenedor
COPY . .

# Exponer el puerto de Axum
EXPOSE 3000

# Comando para correr el servidor en modo desarrollo
CMD ["cargo", "run"]