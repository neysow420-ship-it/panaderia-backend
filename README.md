# Sistema de gestión para panaderías — Backend

API y lógica de negocio de un sistema de gestión para panaderías, desarrollado como proyecto académico de Ingeniería de Sistemas.

## Objetivo

Desarrollar un backend que permita validar y procesar las operaciones del sistema, con énfasis en la consistencia de los datos y el funcionamiento en entornos donde la conectividad puede ser limitada.

## Tecnologías

- **Rust:** lógica de negocio, validaciones y servicios de la API.
- **Axum:** framework para construir la API HTTP.
- **Apache CouchDB:** persistencia central de documentos JSON.
- **Docker Compose:** entorno reproducible de servicios, cuando corresponda.

## Arquitectura

El backend recibe las solicitudes del cliente, valida las operaciones y determina qué información puede persistirse en CouchDB.

Flujo general:

`Frontend → PouchDB → API Rust → Validación → CouchDB`

Los cambios locales pendientes de sincronización deben procesarse según las reglas acordadas para evitar que datos no validados se incorporen a la base central.

## Alcance del sistema

El alcance funcional, los contratos de datos y las reglas de negocio se documentarán conforme avance el proyecto.

## Estado

En desarrollo. Las funcionalidades y los resultados experimentales se documentarán a medida que se implementen y validen.

## Desarrollo

Las instrucciones de instalación, configuración y ejecución se completarán cuando estén definidas la estructura del proyecto y las dependencias.

## Calidad del código

El proyecto utilizará integración continua para verificar el formato del código, detectar problemas de calidad y ejecutar las pruebas automatizadas.

## Trabajo colaborativo

Los cambios se propondrán mediante Pull Requests hacia `main` y deberán cumplir las reglas de revisión y las comprobaciones automatizadas establecidas en el repositorio.
