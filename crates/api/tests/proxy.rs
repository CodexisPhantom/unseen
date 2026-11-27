use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};

use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tower::ServiceExt;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wiremock::matchers::{method, query_param};
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

const SETTINGS: &str = "/api/v1/settings/proxy";
const INDEXERS: &str = "/api/v1/indexers";
const CAPS: &str = include_str!("../../../fixtures/torznab/jackett_caps.xml");
const FEED: &str = include_str!("../../../fixtures/torznab/jackett_movie.xml");
const SECRET: &str = "hunter2-proxy-pass";

/// A proxy that answers a Torznab caps or movie request and records the
/// first line of every request it gets.
async fn indexer_proxy() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buffer = [0; 4096];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buffer[..read]);
                let line = text.lines().next().unwrap_or_default().to_owned();
                let body = if line.contains("t=caps") { CAPS } else { FEED };
                log.lock().unwrap().push(line);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (addr, seen)
}

async fn set_proxy(app: &App, proxy: SocketAddr, bypass_local: bool) {
    let (status, reply) = app
        .owner(
            Method::PUT,
            SETTINGS,
            Some(json!({ "url": format!("http://user:{SECRET}@{proxy}"), "bypassLocal": bypass_local })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
}

async fn indexer(app: &App, url: &str, use_proxy: bool) -> i64 {
    let name = format!("Indexer at {url}");
    let body = json!({ "name": name, "url": url, "apiKey": "key", "useProxy": use_proxy });
    let (status, reply) = app.owner(Method::POST, INDEXERS, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply["id"].as_i64().unwrap()
}

async fn test_indexer(app: &App, id: i64) -> StatusCode {
    app.owner(Method::POST, &format!("{INDEXERS}/{id}/test"), None)
        .await
        .0
}

#[tokio::test]
async fn the_proxy_is_stored_encrypted_and_its_credentials_are_never_returned() {
    let app = App::new().await;
    let (status, reply) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reply,
        json!({
            "hasUrl": false,
            "address": null,
            "hasCredentials": false,
            "urlMustBeReentered": false,
            "bypassLocal": true,
            "overridden": []
        })
    );

    let url = format!("socks5://user:{SECRET}@proxy.example:1080");
    let (status, reply) = app
        .owner(
            Method::PUT,
            SETTINGS,
            Some(json!({ "url": url, "bypassLocal": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["address"], "socks5h://proxy.example:1080");
    assert_eq!(reply["hasCredentials"], true);
    assert_eq!(reply["bypassLocal"], false);
    assert!(!reply.to_string().contains(SECRET));

    // A change without a URL keeps it; the audit log has the fact only.
    let (_, reply) = app
        .owner(Method::PUT, SETTINGS, Some(json!({ "bypassLocal": true })))
        .await;
    assert_eq!(reply["hasUrl"], true);
    let (_, log) = app
        .owner(
            Method::GET,
            "/api/v1/audit-log?action=setting.updated",
            None,
        )
        .await;
    assert_eq!(log["total"], 2);
    assert!(!log.to_string().contains(SECRET));

    // The row decrypts to the URL that was sent.
    let row: Option<String> = db::settings::get_secret(
        &app.state.db,
        &db::secret::SecretKey::derive("test secret"),
        "proxy.url",
    )
    .await
    .unwrap()
    .map(|stored| match stored {
        db::settings::SecretSetting::Set(url) => url.expose().to_owned(),
        db::settings::SecretSetting::MustReenter => String::new(),
    });
    assert_eq!(row.as_deref(), Some(url.as_str()));

    // An empty URL removes the proxy.
    let (_, reply) = app
        .owner(Method::PUT, SETTINGS, Some(json!({ "url": "" })))
        .await;
    assert_eq!(reply["hasUrl"], false);
    assert_eq!(reply["address"], Value::Null);
}

#[tokio::test]
async fn a_bad_proxy_url_is_refused() {
    let app = App::new().await;
    for url in ["ftp://proxy:21", "proxy", "http://169.254.169.254:3128"] {
        let (status, reply) = app
            .owner(Method::PUT, SETTINGS, Some(json!({ "url": url })))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url}: {reply}");
    }
    let (_, reply) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(reply["hasUrl"], false);
}

#[tokio::test]
async fn an_indexer_request_goes_through_the_proxy_unless_the_switch_is_off_or_the_address_is_local()
 {
    let app = App::new().await;
    let (proxy, seen) = indexer_proxy().await;
    set_proxy(&app, proxy, true).await;

    // A name that only the proxy can reach.
    let remote = indexer(&app, "http://indexer.example/api", true).await;
    assert_eq!(test_indexer(&app, remote).await, StatusCode::OK);
    {
        let seen = seen.lock().unwrap();
        assert!(!seen.is_empty());
        assert!(
            seen.iter()
                .all(|line| line.starts_with("GET http://indexer.example/api?")),
            "{seen:?}"
        );
    }

    // The switch off: a direct request, which cannot resolve the name.
    let before = seen.lock().unwrap().len();
    let direct = indexer(&app, "http://indexer.example/direct", false).await;
    assert_eq!(test_indexer(&app, direct).await, StatusCode::BAD_GATEWAY);
    assert_eq!(seen.lock().unwrap().len(), before);

    // A local address with the bypass on is a direct request.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CAPS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
        .mount(&server)
        .await;
    let local = indexer(&app, &server.uri(), true).await;
    assert_eq!(test_indexer(&app, local).await, StatusCode::OK);
    assert_eq!(seen.lock().unwrap().len(), before);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);

    // With the bypass off the same address goes through the proxy.
    set_proxy(&app, proxy, false).await;
    assert_eq!(test_indexer(&app, local).await, StatusCode::OK);
    assert!(seen.lock().unwrap().len() > before);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_unreadable_proxy_stops_indexer_traffic_instead_of_going_direct() {
    let app = App::new().await;
    let server = MockServer::start().await;
    db::settings::set_secret(
        &app.state.db,
        &db::secret::SecretKey::derive("another secret"),
        "proxy.url",
        &db::secret::Secret::new("http://proxy:3128"),
    )
    .await
    .unwrap();
    let id = indexer(&app, &server.uri(), true).await;
    let (status, reply) = app
        .owner(Method::POST, &format!("{INDEXERS}/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "proxy_unreadable");
    assert!(server.received_requests().await.unwrap().is_empty());
    let (_, reply) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(reply["urlMustBeReentered"], true);
}

#[tokio::test]
async fn the_proxy_test_sends_one_request_through_the_proxy() {
    let app = App::new().await;
    let test = format!("{SETTINGS}/test");
    let target = json!({ "url": "http://indexer.example/api?t=caps" });

    let (status, reply) = app.owner(Method::POST, &test, Some(target.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "proxy_not_set");

    let (proxy, seen) = indexer_proxy().await;
    set_proxy(&app, proxy, true).await;
    let (status, reply) = app.owner(Method::POST, &test, Some(target)).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["status"], 200);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["GET http://indexer.example/api?t=caps HTTP/1.1"]
    );

    let (status, _) = app
        .owner(Method::POST, &test, Some(json!({ "url": "ftp://x" })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A proxy that is not listening is a 502 that names the failure.
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);
    set_proxy(&app, dead_addr, true).await;
    let (status, reply) = app
        .owner(
            Method::POST,
            &test,
            Some(json!({ "url": "http://indexer.example/" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{reply}");
    assert_eq!(reply["code"], "proxy_unavailable");
    assert!(!reply.to_string().contains(SECRET));
}

#[tokio::test]
async fn reading_needs_settings_read_and_writing_needs_settings_manage() {
    let app = App::new().await;
    let (_, reader, _) = app.member("reader@example.com", &["settings:read"]).await;
    let (status, _, _) = app.send(Method::GET, SETTINGS, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    for (method, path, payload) in [
        (
            Method::PUT,
            SETTINGS.to_owned(),
            json!({ "bypassLocal": false }),
        ),
        (Method::POST, format!("{SETTINGS}/test"), json!({})),
    ] {
        let (status, _, reply) = app.send(method, &path, Some(&reader), Some(payload)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(reply["requiredPermission"], "settings:manage");
    }
}

#[tokio::test]
async fn an_environment_proxy_wins_and_refuses_changes() {
    let mut app = App::new().await;
    app.state.settings_overrides = db::settings::Overrides::from_vars([(
        "UNSEENARR_SETTING_PROXY_URL".to_owned(),
        format!("http://user:{SECRET}@proxy:3128"),
    )]);
    app.router = router(app.state.clone());

    let (_, reply) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(reply["address"], "http://proxy:3128");
    assert_eq!(reply["overridden"], json!(["url"]));
    assert!(!reply.to_string().contains(SECRET));
    let (status, reply) = app
        .owner(Method::PUT, SETTINGS, Some(json!({ "url": "" })))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "setting_overridden");
    // The other field is still editable.
    let (status, _) = app
        .owner(Method::PUT, SETTINGS, Some(json!({ "bypassLocal": false })))
        .await;
    assert_eq!(status, StatusCode::OK);
}
