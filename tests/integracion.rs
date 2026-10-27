//! Pruebas de integración de la API contra CouchDB real.
//!
//! No se ejecutan con un `cargo test` normal: requieren CouchDB y datos
//! sembrados. El lanzador es:
//!
//! ```text
//! ./tests/ejecutar-integracion.sh
//! ```
//!
//! Cada prueba arranca el binario compilado en un puerto libre, con un JWT de
//! pruebas, y habla con CouchDB usando las credenciales `backend`.

use std::{
    env,
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::Duration,
};

use argon2::{Argon2, password_hash::PasswordHasher};
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, Method, StatusCode, Url};
use serde::Serialize;
use serde_json::{Value, json};
use ulid::Ulid;

const JWT_PRUEBAS: &str = "secreto-de-pruebas-integracion-no-usar-en-produccion";
const PRODUCTO_PAN: &str = "producto:pan-frances";
const PRECIO_PAN: i64 = 10;

// ─────────────────────────────────────────────────────────────────────────────
// Entorno y utilidades
// ─────────────────────────────────────────────────────────────────────────────

struct Entorno {
    couchdb: Url,
    usuario: String,
    contrasena: String,
}

/// Devuelve el entorno solo cuando el lanzador activa las pruebas.
///
/// Así `cargo test` sigue funcionando sin CouchDB para las pruebas unitarias.
fn entorno() -> Option<Entorno> {
    if env::var("PANADERIA_INTEGRACION").as_deref() != Ok("1") {
        eprintln!("Integración omitida: usa ./tests/ejecutar-integracion.sh para ejecutarla.");
        return None;
    }

    let couchdb = env::var("COUCHDB_URL").expect("PANADERIA_INTEGRACION=1 requiere COUCHDB_URL");
    let usuario = env::var("BACKEND_USER").unwrap_or_else(|_| "backend".to_string());
    let contrasena =
        env::var("BACKEND_PASSWORD").expect("PANADERIA_INTEGRACION=1 requiere BACKEND_PASSWORD");

    Some(Entorno {
        couchdb: Url::parse(&couchdb).expect("COUCHDB_URL inválida"),
        usuario,
        contrasena,
    })
}

fn zona() -> FixedOffset {
    FixedOffset::west_opt(6 * 3600).expect("UTC-6 es válido")
}

fn ahora_local() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&zona())
}

fn hoy() -> String {
    ahora_local().format("%Y-%m-%d").to_string()
}

fn creado_ahora() -> String {
    ahora_local().to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn ulid() -> String {
    Ulid::new().to_string()
}

fn nombre_unico(prefijo: &str) -> String {
    format!(
        "prueba-{prefijo}-{}",
        Ulid::new().to_string().to_lowercase()
    )
}

fn puerto_libre() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto libre");
    let puerto = listener.local_addr().expect("dirección local").port();
    drop(listener);
    puerto
}

fn venta_body(operacion_id: &str, cantidad: i64, creado_local: &str) -> Value {
    json!({
        "operacion_id": operacion_id,
        "dispositivo_id": "caja-pruebas",
        "creado_local": creado_local,
        "lineas": [{ "producto_id": PRODUCTO_PAN, "cantidad": cantidad }]
    })
}

fn hornada_body(operacion_id: &str) -> Value {
    json!({
        "operacion_id": operacion_id,
        "producto_id": PRODUCTO_PAN,
        "unidades": 25,
        "creado_local": creado_ahora()
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Cliente directo a CouchDB (para preparar y limpiar documentos)
// ─────────────────────────────────────────────────────────────────────────────

struct Db {
    cliente: Client,
    base: Url,
    usuario: String,
    contrasena: String,
}

impl Db {
    fn new(entorno: &Entorno) -> Self {
        Self {
            cliente: Client::new(),
            base: entorno.couchdb.clone(),
            usuario: entorno.usuario.clone(),
            contrasena: entorno.contrasena.clone(),
        }
    }

    fn url(&self, base: &str, id: &str) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("URL de CouchDB válida")
            .pop_if_empty()
            .extend([base, id]);
        url
    }

    async fn get(&self, base: &str, id: &str) -> Option<Value> {
        let respuesta = self
            .cliente
            .get(self.url(base, id))
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .expect("GET a CouchDB");

        match respuesta.status() {
            StatusCode::OK => Some(respuesta.json().await.expect("JSON de CouchDB")),
            StatusCode::NOT_FOUND => None,
            otro => panic!(
                "CouchDB respondió {otro} al leer {base}/{id}: {}",
                respuesta.text().await.unwrap_or_default()
            ),
        }
    }

    async fn get_path(&self, ruta: &str) -> (StatusCode, Value) {
        let url = format!(
            "{}/{}",
            self.base.as_str().trim_end_matches('/'),
            ruta.trim_start_matches('/')
        );
        let respuesta = self
            .cliente
            .get(url)
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .expect("GET a CouchDB");

        let estado = respuesta.status();
        let cuerpo = respuesta.json().await.unwrap_or(Value::Null);
        (estado, cuerpo)
    }

    /// Crea o reemplaza un documento conservando la `_rev` si ya existía.
    async fn put(&self, base: &str, id: &str, documento: &Value) -> Value {
        let mut cuerpo = documento.clone();

        if let Some(actual) = self.get(base, id).await {
            let rev = actual
                .get("_rev")
                .and_then(Value::as_str)
                .expect("documento existente con _rev");
            cuerpo
                .as_object_mut()
                .expect("documento JSON objeto")
                .insert("_rev".to_string(), json!(rev));
        }

        let respuesta = self
            .cliente
            .put(self.url(base, id))
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .json(&cuerpo)
            .send()
            .await
            .expect("PUT a CouchDB");

        let estado = respuesta.status();
        let texto = respuesta.text().await.unwrap_or_default();

        assert!(
            estado == StatusCode::CREATED || estado == StatusCode::ACCEPTED,
            "CouchDB respondió {estado} al guardar {base}/{id}: {texto}"
        );

        serde_json::from_str(&texto).unwrap_or(Value::Null)
    }

    async fn delete(&self, base: &str, id: &str) {
        let Some(actual) = self.get(base, id).await else {
            return;
        };
        let rev = actual
            .get("_rev")
            .and_then(Value::as_str)
            .expect("documento existente con _rev");

        let mut url = self.url(base, id);
        url.query_pairs_mut().append_pair("rev", rev);

        let respuesta = self
            .cliente
            .delete(url)
            .basic_auth(&self.usuario, Some(&self.contrasena))
            .send()
            .await
            .expect("DELETE a CouchDB");

        assert!(
            respuesta.status().is_success() || respuesta.status() == StatusCode::NOT_FOUND,
            "No se pudo borrar {base}/{id}: {}",
            respuesta.status()
        );
    }

    /// Deja el catálogo canónico listo con los precios del contrato.
    async fn preparar_catalogo(&self) {
        let productos = [
            ("pan-frances", "Pan francés", 10, 1),
            ("semita", "Semita", 25, 2),
            ("hojitas", "Hojitas", 15, 2),
            ("cachos", "Cachos", 30, 2),
            ("donas", "Donas", 35, 2),
        ];

        for (slug, nombre, precio, vida) in productos {
            self.put(
                "catalogo",
                &format!("producto:{slug}"),
                &json!({
                    "tipo": "producto",
                    "nombre": nombre,
                    "precio_centavos": precio,
                    "vida_util_dias": vida,
                    "activo": true
                }),
            )
            .await;
        }
    }

    async fn crear_usuario(&self, nombre: &str, rol: &str, contrasena: &str) {
        let hash = Argon2::default()
            .hash_password(contrasena.as_bytes())
            .expect("hash Argon2id")
            .to_string();

        self.put(
            "usuarios",
            &format!("usuario:{nombre}"),
            &json!({
                "tipo": "usuario",
                "nombre": nombre,
                "rol": rol,
                "hash_argon2": hash,
                "activo": true,
                "creado_en": ahora_local().to_rfc3339_opts(SecondsFormat::Secs, false)
            }),
        )
        .await;
    }

    async fn borrar_usuario(&self, nombre: &str) {
        self.delete("usuarios", &format!("usuario:{nombre}")).await;
    }

    async fn crear_cierre(&self, fecha: &str) {
        self.put(
            "panaderia",
            &format!("cierre:{fecha}"),
            &json!({ "tipo": "cierre", "fecha": fecha }),
        )
        .await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// API en un proceso aparte
// ─────────────────────────────────────────────────────────────────────────────

struct Api {
    hijo: Child,
    base: String,
    cliente: Client,
}

impl Drop for Api {
    fn drop(&mut self) {
        let _ = self.hijo.kill();
        let _ = self.hijo.wait();
    }
}

impl Api {
    async fn iniciar(entorno: &Entorno, auth_mode: &str, cors: Option<&str>) -> Self {
        let puerto = puerto_libre();
        let log = format!("/tmp/panaderia-api-integracion-{puerto}.log");
        let archivo = std::fs::File::create(&log).expect("archivo de log");
        let archivo_error = archivo.try_clone().expect("clonar archivo de log");

        let hijo = Command::new(env!("CARGO_BIN_EXE_panaderia-backend"))
            .env("COUCHDB_URL", entorno.couchdb.as_str())
            .env("BACKEND_USER", &entorno.usuario)
            .env("BACKEND_PASSWORD", &entorno.contrasena)
            .env("PORT", puerto.to_string())
            .env("AUTH_MODE", auth_mode)
            .env("JWT_SECRET", JWT_PRUEBAS)
            .env("CORS_ORIGINS", cors.unwrap_or("http://localhost:5173"))
            .stdout(Stdio::from(archivo))
            .stderr(Stdio::from(archivo_error))
            .spawn()
            .expect("arrancar la API");

        let mut api = Self {
            hijo,
            base: format!("http://127.0.0.1:{puerto}"),
            cliente: Client::new(),
        };

        api.esperar_salud(&log).await;
        api
    }

    async fn esperar_salud(&mut self, log: &str) {
        for _ in 0..100 {
            if let Ok(respuesta) = self
                .cliente
                .get(format!("{}/salud", self.base))
                .send()
                .await
                && respuesta.status().is_success()
            {
                return;
            }

            if let Ok(Some(estado)) = self.hijo.try_wait() {
                let detalle = std::fs::read_to_string(log).unwrap_or_default();
                panic!("La API terminó antes de responder ({estado}):\n{detalle}");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let detalle = std::fs::read_to_string(log).unwrap_or_default();
        panic!("La API no respondió /salud a tiempo:\n{detalle}");
    }

    async fn get(&self, ruta: &str, token: Option<&str>) -> (StatusCode, Value) {
        let mut peticion = self.cliente.get(format!("{}{}", self.base, ruta));
        if let Some(token) = token {
            peticion = peticion.bearer_auth(token);
        }

        let respuesta = peticion.send().await.expect("GET a la API");
        let estado = respuesta.status();
        let cuerpo = respuesta.json().await.unwrap_or(Value::Null);
        (estado, cuerpo)
    }

    async fn post(&self, ruta: &str, token: Option<&str>, cuerpo: &Value) -> (StatusCode, Value) {
        let mut peticion = self
            .cliente
            .post(format!("{}{}", self.base, ruta))
            .json(cuerpo);
        if let Some(token) = token {
            peticion = peticion.bearer_auth(token);
        }

        let respuesta = peticion.send().await.expect("POST a la API");
        let estado = respuesta.status();
        let cuerpo = respuesta.json().await.unwrap_or(Value::Null);
        (estado, cuerpo)
    }

    async fn preflight(&self, ruta: &str, origen: &str) -> reqwest::Response {
        self.cliente
            .request(Method::OPTIONS, format!("{}{}", self.base, ruta))
            .header("Origin", origen)
            .header("Access-Control-Request-Method", "POST")
            .header(
                "Access-Control-Request-Headers",
                "content-type,authorization",
            )
            .send()
            .await
            .expect("OPTIONS a la API")
    }

    async fn login(&self, usuario: &str, contrasena: &str) -> (StatusCode, Value) {
        self.post(
            "/auth/login",
            None,
            &json!({ "usuario": usuario, "contrasena": contrasena }),
        )
        .await
    }
}

fn token_de(respuesta: &Value) -> String {
    respuesta["token"]
        .as_str()
        .expect("respuesta de login con token")
        .to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Pruebas
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn salud_login_y_cors() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;

    let usuario = nombre_unico("cajero");
    let contrasena = "contrasena-de-prueba-123";
    db.crear_usuario(&usuario, "cajero", contrasena).await;

    let api = Api::iniciar(&entorno, "activo", Some("https://panaderia.duckdns.org")).await;

    let (estado, cuerpo) = api.get("/salud", None).await;
    assert_eq!(estado, StatusCode::OK);
    assert_eq!(cuerpo["estado"], "ok");

    let (estado, _) = api.login(&usuario, "contrasena-equivocada").await;
    assert_eq!(estado, StatusCode::UNAUTHORIZED);

    let (estado, respuesta) = api.login(&usuario, contrasena).await;
    assert_eq!(estado, StatusCode::OK);
    assert_eq!(respuesta["usuario"]["rol"], "cajero");
    assert_eq!(respuesta["tipo"], "Bearer");
    let token = token_de(&respuesta);

    let (estado, cuerpo) = api.get("/api/yo", Some(&token)).await;
    assert_eq!(estado, StatusCode::OK);
    assert_eq!(cuerpo["rol"], "cajero");

    let preflight = api
        .preflight("/ventas", "https://panaderia.duckdns.org")
        .await;
    assert!(preflight.status().is_success());
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-origin")
            .and_then(|valor| valor.to_str().ok()),
        Some("https://panaderia.duckdns.org")
    );

    db.borrar_usuario(&usuario).await;
}

#[tokio::test]
async fn token_ausente_o_expirado_es_401() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;
    let api = Api::iniciar(&entorno, "activo", None).await;

    let (estado, _) = api.get("/api/yo", None).await;
    assert_eq!(estado, StatusCode::UNAUTHORIZED);

    let (estado, _) = api.get("/api/yo", Some("token-invalido")).await;
    assert_eq!(estado, StatusCode::UNAUTHORIZED);

    #[derive(Serialize)]
    struct Claims {
        sub: String,
        rol: &'static str,
        exp: usize,
    }

    let exp = (Utc::now() - chrono::Duration::hours(1)).timestamp() as usize;
    let expirado = encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: "cajero".to_string(),
            rol: "cajero",
            exp,
        },
        &EncodingKey::from_secret(JWT_PRUEBAS.as_bytes()),
    )
    .expect("token expirado de prueba");

    let (estado, _) = api.get("/api/yo", Some(&expirado)).await;
    assert_eq!(estado, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn permisos_por_rol() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;

    let cajero = nombre_unico("cajero");
    let encargado = nombre_unico("encargado");
    let contrasena = "contrasena-de-prueba-123";
    db.crear_usuario(&cajero, "cajero", contrasena).await;
    db.crear_usuario(&encargado, "encargado", contrasena).await;

    let api = Api::iniciar(&entorno, "activo", None).await;

    let (_, respuesta) = api.login(&cajero, contrasena).await;
    let token_cajero = token_de(&respuesta);
    let (_, respuesta) = api.login(&encargado, contrasena).await;
    let token_encargado = token_de(&respuesta);

    let operacion_venta = ulid();
    let (estado, _) = api
        .post(
            "/ventas",
            Some(&token_cajero),
            &venta_body(&operacion_venta, 1, &creado_ahora()),
        )
        .await;
    assert_eq!(estado, StatusCode::CREATED);

    let operacion_hornada = ulid();
    let (estado, _) = api
        .post(
            "/hornadas",
            Some(&token_cajero),
            &hornada_body(&operacion_hornada),
        )
        .await;
    assert_eq!(estado, StatusCode::FORBIDDEN);

    let hoy = hoy();
    let (estado, _) = api
        .get(
            &format!("/reportes/ventas?desde={hoy}&hasta={hoy}"),
            Some(&token_cajero),
        )
        .await;
    assert_eq!(estado, StatusCode::FORBIDDEN);

    let (estado, _) = api
        .get(
            &format!("/reportes/ventas?desde={hoy}&hasta={hoy}"),
            Some(&token_encargado),
        )
        .await;
    assert_eq!(estado, StatusCode::OK);

    let (estado, _) = api
        .post(
            "/hornadas",
            Some(&token_encargado),
            &hornada_body(&operacion_hornada),
        )
        .await;
    assert_eq!(estado, StatusCode::CREATED);

    db.delete("panaderia", &format!("venta:{operacion_venta}"))
        .await;
    db.delete("panaderia", &format!("hornada:{operacion_hornada}"))
        .await;
    db.borrar_usuario(&cajero).await;
    db.borrar_usuario(&encargado).await;
}

#[tokio::test]
async fn venta_idempotente_y_centavos() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;

    let cajero = nombre_unico("cajero");
    let contrasena = "contrasena-de-prueba-123";
    db.crear_usuario(&cajero, "cajero", contrasena).await;

    let api = Api::iniciar(&entorno, "activo", None).await;
    let (_, respuesta) = api.login(&cajero, contrasena).await;
    let token = token_de(&respuesta);

    let operacion = ulid();
    let cuerpo = venta_body(&operacion, 3, &creado_ahora());

    let (estado, respuesta) = api.post("/ventas", Some(&token), &cuerpo).await;
    assert_eq!(estado, StatusCode::CREATED);
    assert_eq!(respuesta["ya_procesada"], false);
    assert_eq!(respuesta["venta"]["total_centavos"], PRECIO_PAN * 3);
    assert_eq!(
        respuesta["venta"]["lineas"][0]["precio_centavos"],
        PRECIO_PAN
    );
    assert_eq!(
        respuesta["venta"]["lineas"][0]["subtotal_centavos"],
        PRECIO_PAN * 3
    );

    let (estado, respuesta) = api.post("/ventas", Some(&token), &cuerpo).await;
    assert_eq!(estado, StatusCode::OK);
    assert_eq!(respuesta["ya_procesada"], true);
    assert_eq!(respuesta["venta"]["total_centavos"], PRECIO_PAN * 3);

    let (estado, respuesta) = api
        .post(
            "/ventas",
            Some(&token),
            &venta_body(&operacion, 4, &creado_ahora()),
        )
        .await;
    assert_eq!(estado, StatusCode::CONFLICT);
    assert_eq!(respuesta["codigo"], "operacion_en_conflicto");

    db.delete("panaderia", &format!("venta:{operacion}")).await;
    db.borrar_usuario(&cajero).await;
}

#[tokio::test]
async fn validaciones_de_venta_son_422() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;

    let cajero = nombre_unico("cajero");
    let contrasena = "contrasena-de-prueba-123";
    db.crear_usuario(&cajero, "cajero", contrasena).await;

    let api = Api::iniciar(&entorno, "activo", None).await;
    let (_, respuesta) = api.login(&cajero, contrasena).await;
    let token = token_de(&respuesta);

    let (estado, cuerpo) = api
        .post(
            "/ventas",
            Some(&token),
            &venta_body(&ulid(), 0, &creado_ahora()),
        )
        .await;
    assert_eq!(estado, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cuerpo["codigo"], "validacion");

    let (estado, _) = api
        .post(
            "/ventas",
            Some(&token),
            &json!({
                "operacion_id": ulid(),
                "dispositivo_id": "caja-pruebas",
                "creado_local": "2026-10-13T08:15:32",
                "lineas": [{ "producto_id": PRODUCTO_PAN, "cantidad": 1 }]
            }),
        )
        .await;
    assert_eq!(estado, StatusCode::UNPROCESSABLE_ENTITY);

    let (estado, _) = api
        .post(
            "/ventas",
            Some(&token),
            &json!({
                "operacion_id": ulid(),
                "dispositivo_id": "caja-pruebas",
                "creado_local": creado_ahora(),
                "lineas": [{ "producto_id": "producto:no-existe", "cantidad": 1 }]
            }),
        )
        .await;
    assert_eq!(estado, StatusCode::UNPROCESSABLE_ENTITY);

    db.borrar_usuario(&cajero).await;
}

#[tokio::test]
async fn venta_de_dia_cerrado_conserva_fecha_original() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;

    let cajero = nombre_unico("cajero");
    let contrasena = "contrasena-de-prueba-123";
    db.crear_usuario(&cajero, "cajero", contrasena).await;

    let fecha_vieja = "2020-01-02";
    db.crear_cierre(fecha_vieja).await;

    let api = Api::iniciar(&entorno, "activo", None).await;
    let (_, respuesta) = api.login(&cajero, contrasena).await;
    let token = token_de(&respuesta);

    let operacion = ulid();
    let (estado, respuesta) = api
        .post(
            "/ventas",
            Some(&token),
            &venta_body(&operacion, 1, "2020-01-02T19:50:00-06:00"),
        )
        .await;

    assert_eq!(estado, StatusCode::CREATED);
    assert_eq!(respuesta["venta"]["fecha_original"], fecha_vieja);
    assert!(
        respuesta["venta"]["fecha"]
            .as_str()
            .is_some_and(|fecha| fecha >= hoy().as_str())
    );

    db.delete("panaderia", &format!("venta:{operacion}")).await;
    db.delete("panaderia", &format!("cierre:{fecha_vieja}"))
        .await;
    db.borrar_usuario(&cajero).await;
}

#[tokio::test]
async fn auth_desactivado_no_exige_token() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);
    db.preparar_catalogo().await;
    let api = Api::iniciar(&entorno, "desactivado", None).await;

    let (estado, cuerpo) = api.get("/api/yo", None).await;
    assert_eq!(estado, StatusCode::OK);
    assert_eq!(cuerpo["rol"], "encargado");

    let operacion = ulid();
    let (estado, _) = api.post("/hornadas", None, &hornada_body(&operacion)).await;
    assert_eq!(estado, StatusCode::CREATED);

    db.delete("panaderia", &format!("hornada:{operacion}"))
        .await;
}

#[tokio::test]
async fn vistas_e_indices_estan_instalados() {
    let Some(entorno) = entorno() else {
        return;
    };
    let db = Db::new(&entorno);

    let (estado, _) = db.get_path("panaderia/_design/ventas").await;
    assert_eq!(estado, StatusCode::OK);

    let (estado, _) = db.get_path("panaderia/_design/hornadas").await;
    assert_eq!(estado, StatusCode::OK);

    let (estado, _) = db
        .get_path("panaderia/_design/ventas/_view/ventas_por_dia_producto?limit=0")
        .await;
    assert_eq!(estado, StatusCode::OK);

    let (estado, _) = db
        .get_path("panaderia/_design/hornadas/_view/hornadas_por_dia_producto?limit=0")
        .await;
    assert_eq!(estado, StatusCode::OK);

    let (estado, cuerpo) = db.get_path("panaderia/_index").await;
    assert_eq!(estado, StatusCode::OK);

    let nombres: Vec<&str> = cuerpo["indexes"]
        .as_array()
        .expect("lista de índices")
        .iter()
        .filter_map(|indice| indice["name"].as_str())
        .collect();

    for esperado in [
        "ventas-por-fecha",
        "ventas-por-producto",
        "hornadas-por-fecha",
        "hornadas-por-producto",
    ] {
        assert!(
            nombres.contains(&esperado),
            "Falta el índice {esperado}; ejecuta infra/couchdb/setup.sh"
        );
    }
}
