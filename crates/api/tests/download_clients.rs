use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};

use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "unseenarr.test";
const ORIGIN: &str = "http://unseenarr.test";
const PASSWORD: &str = "correct horse";

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

impl App {
    async fn new() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        let mut app = Self {
            router: router(state.clone()),
            state,
            owner: String::new(),
            _dir: dir,
        };
        let body = json!({ "name": "Owner", "email": "owner@example.com", "password": PASSWORD });
        let (status, cookie, reply) = app
            .send(Method::POST, "/api/v1/setup", None, Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        app.owner = cookie.unwrap();
        app
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .find(|cookie| cookie.contains("session_token="))
            .map(|cookie| cookie.split(';').next().unwrap().to_owned());
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            cookie,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn owner(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (status, _, body) = self.send(method, path, Some(&self.owner), body).await;
        (status, body)
    }

    /// A user who holds a new role with `permissions`. Returns the user's
    /// cookie and the role id.
    async fn member(&self, email: &str, permissions: &[&str]) -> (String, String, i32) {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let (status, cookie, reply) = self
            .send(Method::POST, "/api/auth/sign-up/email", None, Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let user_id = reply["user"]["id"].as_str().unwrap().to_owned();
        let role = roles::ActiveModel {
            name: Set(format!("Role of {email}")),
            is_owner: Set(false),
            created_at: Set(db::now()),
            updated_at: Set(db::now()),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        for permission in permissions {
            role_permissions::ActiveModel {
                role_id: Set(role.id),
                permission: Set((*permission).into()),
            }
            .insert(&self.state.db)
            .await
            .unwrap();
        }
        user_roles::ActiveModel {
            user_id: Set(user_id.clone()),
            role_id: Set(role.id),
            all_libraries: Set(true),
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        (user_id, cookie.unwrap(), role.id)
    }
}

const PATH: &str = "/api/v1/download-clients";
const SECRET: &str = "qb-s3cret-pass";

fn body(name: &str) -> Value {
    json!({
        "name": name,
        "url": "http://qbittorrent.test:8080",
        "username": "admin",
        "password": SECRET,
    })
}

async fn create(app: &App, name: &str) -> i64 {
    let (status, reply) = app.owner(Method::POST, PATH, Some(body(name))).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply["id"].as_i64().unwrap()
}

async fn stored_password(app: &App, id: i64) -> Option<String> {
    db::download_clients::find(&app.state.db, id as i32)
        .await
        .unwrap()
        .unwrap()
        .model
        .password
}

#[tokio::test]
async fn create_list_update_and_delete() {
    let app = App::new().await;
    let id = create(&app, "Beta").await;
    create(&app, "Alpha").await;

    let (status, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<_> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Alpha", "Beta"]);
    let beta = &reply[1];
    assert_eq!(beta["kind"], "qbittorrent");
    assert_eq!(beta["category"], "radarr");
    assert_eq!(beta["username"], "admin");
    assert_eq!(beta["hasPassword"], true);
    assert_eq!(beta["passwordMustBeReentered"], false);
    assert_eq!(beta["enabled"], true);
    assert_eq!(beta["remotePath"], Value::Null);

    let path = format!("{PATH}/{id}");
    let (status, reply) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({
                "category": "movies",
                "enabled": false,
                "remotePath": "/downloads",
                "localPath": "/data/downloads",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["category"], "movies");
    assert_eq!(reply["enabled"], false);
    assert_eq!(reply["remotePath"], "/downloads");
    assert_eq!(reply["localPath"], "/data/downloads");

    // An empty string clears the mapping.
    let (_, reply) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "remotePath": "", "localPath": "" })),
        )
        .await;
    assert_eq!(reply["remotePath"], Value::Null);

    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_password_is_never_returned_or_logged_and_a_rename_keeps_it() {
    let app = App::new().await;
    let id = create(&app, "Alpha").await;
    let path = format!("{PATH}/{id}");

    let before = stored_password(&app, id).await.unwrap();
    assert!(!before.contains(SECRET), "stored encrypted");

    let (status, renamed) = app
        .owner(Method::PATCH, &path, Some(json!({ "name": "Renamed" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored_password(&app, id).await.unwrap(), before);
    assert_eq!(
        db::secret::decrypt(&app.state.secrets, &before)
            .unwrap()
            .expose(),
        SECRET
    );

    let (_, listed) = app.owner(Method::GET, PATH, None).await;
    let (_, created) = app.owner(Method::POST, PATH, Some(body("Other"))).await;
    let (status, changed) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "password": "fresh-pass-1" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, audit) = app.owner(Method::GET, "/api/v1/audit-log", None).await;
    assert_eq!(status, StatusCode::OK);
    for reply in [&renamed, &listed, &created, &changed, &audit] {
        let text = reply.to_string();
        assert!(
            !text.contains(SECRET) && !text.contains("fresh-pass-1"),
            "{text}"
        );
    }
    assert!(audit.to_string().contains("download_client.updated"));

    let (_, cleared) = app
        .owner(Method::PATCH, &path, Some(json!({ "password": "" })))
        .await;
    assert_eq!(cleared["hasPassword"], false);
    assert_eq!(stored_password(&app, id).await, None);
}

#[tokio::test]
async fn rejects_bad_input_and_duplicate_names() {
    let app = App::new().await;
    create(&app, "Alpha").await;
    let (status, reply) = app.owner(Method::POST, PATH, Some(body("Alpha"))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "name_taken");

    for (field, bad) in [
        ("name", json!({ "name": " " })),
        ("url", json!({ "url": "ftp://x.test/" })),
        ("url", json!({ "url": "http://169.254.169.254/" })),
        ("category", json!({ "category": " " })),
        ("kind", json!({ "kind": "transmission" })),
        ("apiKey", json!({ "kind": "sabnzbd", "apiKey": "" })),
        ("remotePath", json!({ "remotePath": "/downloads" })),
        ("tags", json!({ "tags": [999] })),
    ] {
        let mut request = body("New");
        request
            .as_object_mut()
            .unwrap()
            .extend(bad.as_object().unwrap().clone());
        let (status, reply) = app.owner(Method::POST, PATH, Some(request)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {reply}");
        assert_eq!(reply["errors"][0]["field"], field);
    }
}

async fn client_at(app: &App, server: &MockServer, password: &str) -> i64 {
    let mut request = body("Local");
    request["url"] = json!(server.uri());
    request["password"] = json!(password);
    let (status, reply) = app.owner(Method::POST, PATH, Some(request)).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply["id"].as_i64().unwrap()
}

#[tokio::test]
async fn the_test_button_reports_the_version() {
    let app = App::new().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "SID=abc; path=/")
                .set_body_string("Ok."),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/app/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string("v4.6.2"))
        .mount(&server)
        .await;
    let id = client_at(&app, &server, "good").await;
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["version"], "v4.6.2");
}

#[tokio::test]
async fn the_test_button_names_wrong_credentials() {
    let app = App::new().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Fails."))
        .mount(&server)
        .await;
    let id = client_at(&app, &server, "wrong").await;
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(reply["code"], "client_credentials_rejected");
    assert!(!reply.to_string().contains("wrong"));
}

#[tokio::test]
async fn the_test_button_reports_an_unreachable_client_and_unknown_ids() {
    let app = App::new().await;
    let server = MockServer::start().await;
    let id = client_at(&app, &server, "x").await;
    drop(server);
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(reply["code"], "client_unavailable");

    let (status, _) = app
        .owner(Method::POST, &format!("{PATH}/999/test"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_unreadable_stored_password_must_be_entered_again() {
    let app = App::new().await;
    let server = MockServer::start().await;
    let id = client_at(&app, &server, "x").await;
    let other = db::secret::SecretKey::derive("another root secret");
    let foreign = db::secret::encrypt(&other, &db::secret::Secret::new("x"));
    let mut model: db::entities::download_clients::ActiveModel =
        db::entities::download_clients::Entity::find_by_id(id as i32)
            .one(&app.state.db)
            .await
            .unwrap()
            .unwrap()
            .into();
    model.password = Set(Some(foreign));
    model.update(&app.state.db).await.unwrap();

    let (_, listed) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(listed[0]["hasPassword"], false);
    assert_eq!(listed[0]["passwordMustBeReentered"], true);
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "client_password_unreadable");
}

#[tokio::test]
async fn reading_needs_clients_read_and_writing_needs_clients_manage() {
    let app = App::new().await;
    let id = create(&app, "Alpha").await;
    let (_, reader, _) = app.member("reader@example.com", &["clients:read"]).await;
    let path = format!("{PATH}/{id}");

    let (status, _, _) = app.send(Method::GET, PATH, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    for (method, path, payload) in [
        (Method::POST, PATH.to_owned(), Some(body("New"))),
        (Method::PATCH, path.clone(), Some(json!({ "name": "x" }))),
        (Method::DELETE, path.clone(), None),
        (Method::POST, format!("{path}/test"), None),
    ] {
        let (status, _, reply) = app.send(method, &path, Some(&reader), payload).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(reply["requiredPermission"], "clients:manage");
    }
}

const API_KEY: &str = "qbt_abcdefghijklmnopqrstuvwxyz12";

#[tokio::test]
async fn an_api_key_is_write_only_and_the_test_uses_it_without_a_login() {
    let app = App::new().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/app/version"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {API_KEY}").as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("v5.2.0"))
        .mount(&server)
        .await;

    let mut request = body("Keyed");
    request["url"] = json!(server.uri());
    request["apiKey"] = json!(API_KEY);
    let (status, created) = app.owner(Method::POST, PATH, Some(request)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["hasApiKey"], true);
    assert_eq!(created["apiKeyMustBeReentered"], false);
    let id = created["id"].as_i64().unwrap();

    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["version"], "v5.2.0");

    // A rename keeps the key; the key is in no response and no audit entry.
    let (_, renamed) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{id}"),
            Some(json!({ "name": "Renamed" })),
        )
        .await;
    let (_, listed) = app.owner(Method::GET, PATH, None).await;
    let (_, audit) = app.owner(Method::GET, "/api/v1/audit-log", None).await;
    for reply in [&created, &renamed, &listed, &audit] {
        assert!(!reply.to_string().contains(API_KEY), "{reply}");
    }
    assert_eq!(renamed["hasApiKey"], true);
    let (status, _) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);

    // An empty string removes the key.
    let (_, cleared) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{id}"),
            Some(json!({ "apiKey": "" })),
        )
        .await;
    assert_eq!(cleared["hasApiKey"], false);
}

#[tokio::test]
async fn a_refused_api_key_is_reported_and_an_unreadable_one_must_be_entered_again() {
    let app = App::new().await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/app/version"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let mut request = body("Keyed");
    request["url"] = json!(server.uri());
    request["apiKey"] = json!("qbt_wrong");
    let (_, created) = app.owner(Method::POST, PATH, Some(request)).await;
    let id = created["id"].as_i64().unwrap();

    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(reply["code"], "client_api_key_rejected");

    let other = db::secret::SecretKey::derive("another root secret");
    let mut model: db::entities::download_clients::ActiveModel =
        db::entities::download_clients::Entity::find_by_id(id as i32)
            .one(&app.state.db)
            .await
            .unwrap()
            .unwrap()
            .into();
    model.api_key = Set(Some(db::secret::encrypt(
        &other,
        &db::secret::Secret::new("x"),
    )));
    model.update(&app.state.db).await.unwrap();
    let (_, listed) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(listed[0]["hasApiKey"], false);
    assert_eq!(listed[0]["apiKeyMustBeReentered"], true);
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "client_api_key_unreadable");
}

const SAB_KEY: &str = "sab-secret-key";

#[tokio::test]
async fn sabnzbd_is_created_with_its_api_key_write_only_and_tested() {
    let app = App::new().await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .and(wiremock::matchers::query_param("mode", "queue"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"queue":{"slots":[]}}"#))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .and(wiremock::matchers::query_param("mode", "version"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":"4.3.2"}"#))
        .expect(1)
        .mount(&server)
        .await;

    let (status, created) = app
        .owner(
            Method::POST,
            PATH,
            Some(json!({
                "name": "Sab",
                "kind": "sabnzbd",
                "url": server.uri(),
                "apiKey": SAB_KEY,
                "username": "ignored",
                "password": "ignored",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["kind"], "sabnzbd");
    assert_eq!(created["hasApiKey"], true);
    assert_eq!(created["apiKeyMustBeReentered"], false);
    assert_eq!(created["username"], Value::Null);
    assert_eq!(created["hasPassword"], false);
    let id = created["id"].as_i64().unwrap();

    let stored = db::download_clients::find(&app.state.db, id as i32)
        .await
        .unwrap()
        .unwrap()
        .model;
    let encrypted_key = stored.api_key.clone().unwrap();
    assert!(!encrypted_key.contains(SAB_KEY));
    assert_eq!(
        db::secret::decrypt(&app.state.secrets, &encrypted_key)
            .unwrap()
            .expose(),
        SAB_KEY
    );
    assert_eq!(stored.username, None);
    assert_eq!(stored.password, None);

    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["version"], "4.3.2");

    let (_, renamed) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{id}"),
            Some(json!({ "name": "Sab2" })),
        )
        .await;
    assert_eq!(renamed["hasApiKey"], true);
    let (_, listed) = app.owner(Method::GET, PATH, None).await;
    let (status, audit) = app.owner(Method::GET, "/api/v1/audit-log", None).await;
    assert_eq!(status, StatusCode::OK);
    for reply in [&renamed, &listed, &audit] {
        assert!(!reply.to_string().contains(SAB_KEY), "{reply}");
    }

    let (status, _) = app
        .owner(Method::DELETE, &format!("{PATH}/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app
        .owner(Method::DELETE, &format!("{PATH}/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_sabnzbd_key_is_required_and_a_wrong_one_is_named() {
    let app = App::new().await;

    let (status, reply) = app
        .owner(
            Method::POST,
            PATH,
            Some(json!({
                "name": "NoKey",
                "kind": "sabnzbd",
                "url": "http://sabnzbd.test:8080",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["errors"][0]["field"], "apiKey");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .respond_with(ResponseTemplate::new(401).set_body_string("error: API Key Incorrect"))
        .mount(&server)
        .await;
    let (status, created) = app
        .owner(
            Method::POST,
            PATH,
            Some(json!({
                "name": "Sab",
                "kind": "sabnzbd",
                "url": server.uri(),
                "apiKey": "wrong-key",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{reply}");
    assert_eq!(reply["code"], "client_api_key_rejected");

    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{id}"),
            Some(json!({ "apiKey": "" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["errors"][0]["field"], "apiKey");
}
