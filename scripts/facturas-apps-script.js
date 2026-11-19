// Copia este archivo en Code.gs y agrega el servicio avanzado Gmail API.
function importarFacturas() {
  const bloqueo = LockService.getScriptLock();
  if (!bloqueo.tryLock(1000)) return;

  try {
    const propiedades = PropertiesService.getScriptProperties();
    const url = (propiedades.getProperty('API_URL') || '').replace(/\/+$/, '');
    const clave = propiedades.getProperty('FACTURAS_IMPORTACION_CLAVE');
    const remitente = (propiedades.getProperty('REMITENTE') || '').trim().toLowerCase();
    if (!url.startsWith('https://') || !clave || !remitente) {
      throw new Error('Configura API_URL (HTTPS), FACTURAS_IMPORTACION_CLAVE y REMITENTE.');
    }

    const etiquetas = Gmail.Users.Labels.list('me').labels || [];
    const entrada = etiquetas.find((etiqueta) => etiqueta.name === 'facturas-panaderia');
    if (!entrada) throw new Error('Crea la etiqueta facturas-panaderia en Gmail.');
    const procesadas = etiquetas.find((etiqueta) => etiqueta.name === 'facturas-importadas') ||
      Gmail.Users.Labels.create({ name: 'facturas-importadas' }, 'me');

    const pendientes = [];
    let pagina;
    do {
      const parametros = {
        labelIds: [entrada.id],
        q: '-label:facturas-importadas',
        maxResults: 100,
      };
      if (pagina) parametros.pageToken = pagina;
      const resultado = Gmail.Users.Messages.list('me', parametros);
      pendientes.push(...(resultado.messages || []));
      pagina = resultado.nextPageToken;
    } while (pagina);

    let fallos = 0;
    for (const { id } of pendientes) {
      try {
        const mensaje = GmailApp.getMessageById(id);
        const desde = mensaje.getFrom().trim();
        const direccion = (desde.match(/<([^<>]+)>$/)?.[1] || desde).toLowerCase();
        if (direccion !== remitente) continue;

        const adjuntos = mensaje.getAttachments({ includeInlineImages: false })
          .filter((archivo) => archivo.getName().toLowerCase().endsWith('.json'));
        if (adjuntos.length !== 1 || adjuntos[0].getSize() > 16 * 1024) {
          console.warn('Adjunto inválido en el mensaje ' + id);
          continue;
        }

        const respuesta = UrlFetchApp.fetch(url + '/facturas/importar', {
          method: 'post',
          contentType: 'application/json',
          headers: { Authorization: 'Bearer ' + clave },
          payload: adjuntos[0].getDataAsString('UTF-8'),
          followRedirects: false,
          muteHttpExceptions: true,
        });
        const estado = respuesta.getResponseCode();
        if (estado !== 200 && estado !== 201) {
          console.warn('Importación HTTP ' + estado + ' para el mensaje ' + id);
          fallos++;
          continue;
        }

        Gmail.Users.Messages.modify({ addLabelIds: [procesadas.id] }, 'me', id);
        console.log('Factura procesada, HTTP ' + estado + ', mensaje ' + id);
      } catch (_) {
        console.warn('Falló el procesamiento del mensaje ' + id);
        fallos++;
      }
    }
    if (fallos) throw new Error(fallos + ' mensaje(s) pendientes por error de importación.');
  } finally {
    bloqueo.releaseLock();
  }
}
