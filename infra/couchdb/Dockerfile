# Imagen de CouchDB con la configuración de CORS incluida.
#
# Se copia dentro de la imagen (y no como bind mount) para que el entrypoint
# oficial pueda ajustar permisos sin fallar con archivos de solo lectura.
FROM couchdb:3.3.3

COPY --chown=couchdb:couchdb local.d/10-cors.ini /opt/couchdb/etc/local.d/10-cors.ini
