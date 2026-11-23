use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header, request};
use better_auth::seaorm::sea_orm::EntityTrait;
use db::auth_schema::{account, session, user};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";
const EMAIL: &str = "owner@example.com";
const PASSWORD: &str = "correct horse";

struct App {
    router: Router,
    /// What a browser sends as `Origin`.
    origin: String,
    state: AppState,
    _dir: TempDir,
}

struct Reply {
    status: StatusCode,
    /// Every `Set-Cookie` header of the response.
    cookies: Vec<String>,
    body: Value,
}

impl Reply {
    /// The `Set-Cookie` header of the session token.
    fn session_cookie(&self) -> &str {
        self.cookies
            .iter()
            .find(|cookie| cookie.contains("session_token="))
            .expect("a session cookie")
    }

    /// `name=value` of the session cookie, as a browser sends it back.
    fn session_pair(&self) -> &str {
        self.session_cookie().split(';').next().unwrap()
    }
}

impl App {
    async fn new(base_url: Option<&str>) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", base_url).await.unwrap();
        Self {
            router: router(state.clone()),
            origin: base_url.unwrap_or("http://unseen.test").to_owned(),
            state,
            _dir: dir,
        }
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let cookies = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            cookies,
            body,
        }
    }

    /// A request as a browser on the app's own origin sends it.
    fn request(&self, method: &str, path: &str, cookie: Option<&str>) -> request::Builder {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request
                .header(header::COOKIE, cookie)
                .header(header::ORIGIN, &self.origin);
        }
        request
    }

    async fn post(&self, path: &str, body: Value, cookie: Option<&str>) -> Reply {
        let request = self
            .request("POST", path, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    async fn get(&self, path: &str, cookie: Option<&str>) -> Reply {
        let request = self.request("GET", path, cookie).body(Body::empty());
        self.send(request.unwrap()).await
    }

    async fn sign_up(&self, password: &str) -> Reply {
        let body = json!({ "name": "Owner", "email": EMAIL, "password": password });
        self.post("/api/auth/sign-up/email", body, None).await
    }

    /// Creates the Owner through the first-run setup. Returns the user id.
    async fn seed_user(&self) -> String {
        let body = json!({ "name": "Owner", "email": EMAIL, "password": PASSWORD });
        let reply = self.post("/api/v1/setup", body, None).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        reply.body["id"].as_str().unwrap().to_owned()
    }

    async fn sign_in(&self, password: &str) -> Reply {
        let body = json!({ "email": EMAIL, "password": password });
        self.post("/api/auth/sign-in/email", body, None).await
    }
}

#[tokio::test]
async fn auth_routes_are_mounted() {
    let app = App::new(None).await;
    app.seed_user().await;
    let reply = app.get("/api/auth/ok", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["ok"], true);
}

#[tokio::test]
async fn public_sign_up_is_refused() {
    let app = App::new(None).await;
    // Before setup the auth routes answer `setup_required`.
    let reply = app.sign_up(PASSWORD).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["code"], "setup_required");
    assert!(reply.cookies.is_empty());
    assert!(
        user::Entity::find()
            .all(&app.state.db)
            .await
            .unwrap()
            .is_empty()
    );

    // After setup, sign-up stays closed.
    app.seed_user().await;
    let body = json!({ "name": "Second", "email": "second@example.com", "password": PASSWORD });
    let reply = app.post("/api/auth/sign-up/email", body, None).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.body["code"], "signup_closed");
    assert_eq!(
        user::Entity::find().all(&app.state.db).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn sign_in_session_read_and_sign_out() {
    let app = App::new(None).await;
    app.seed_user().await;

    let signed_in = app.sign_in(PASSWORD).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);
    let cookie = signed_in.session_cookie();
    assert!(cookie.starts_with("better-auth.session_token="), "{cookie}");
    for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=2592000"] {
        assert!(cookie.contains(attribute), "{attribute} missing: {cookie}");
    }
    assert!(!cookie.contains("Secure"), "{cookie}");
    let pair = signed_in.session_pair();

    let read = app.get("/api/auth/get-session", Some(pair)).await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body["user"]["email"], EMAIL);

    let signed_out = app.post("/api/auth/sign-out", json!({}), Some(pair)).await;
    assert_eq!(signed_out.status, StatusCode::OK, "{}", signed_out.body);
    let cleared = signed_out.session_cookie();
    assert!(cleared.contains("Max-Age=0"), "{cleared}");

    // The session is gone on the server, not only in the browser.
    let read = app.get("/api/auth/get-session", Some(pair)).await;
    assert_eq!(read.body, Value::Null);
}

#[tokio::test]
async fn session_lasts_thirty_days() {
    let app = App::new(None).await;
    app.seed_user().await;
    let sessions = session::Entity::find().all(&app.state.db).await.unwrap();
    let lifetime = sessions[0].expires_at - sessions[0].created_at;
    assert_eq!(lifetime.num_days(), 30);
}

#[tokio::test]
async fn session_read_without_a_cookie_returns_no_session() {
    let app = App::new(None).await;
    app.seed_user().await;
    let read = app.get("/api/auth/get-session", None).await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body, Value::Null);
}

#[tokio::test]
async fn wrong_password_is_refused() {
    let app = App::new(None).await;
    app.seed_user().await;
    let reply = app.sign_in("wrong password").await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(reply.cookies.is_empty());
}

#[tokio::test]
async fn disabled_user_cannot_sign_in() {
    let app = App::new(None).await;
    let user_id = app.seed_user().await;
    app.state.policy.disable(&user_id);
    let reply = app.sign_in(PASSWORD).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.body["code"], "FAILED_TO_CREATE_SESSION");
    assert!(reply.cookies.is_empty());
}

#[tokio::test]
async fn password_hash_is_argon2() {
    let app = App::new(None).await;
    app.seed_user().await;
    let accounts = account::Entity::find().all(&app.state.db).await.unwrap();
    let hash = accounts[0].password.as_deref().unwrap();
    assert!(hash.starts_with("$argon2id$"), "{hash}");
}

#[tokio::test]
async fn cookie_is_secure_when_the_base_url_is_https() {
    let app = App::new(Some("https://unseen.example.com")).await;
    app.seed_user().await;
    let reply = app.sign_in(PASSWORD).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let cookie = reply.session_cookie();
    assert!(cookie.starts_with("__Secure-better-auth.session_token="));
    for attribute in ["Secure", "HttpOnly", "SameSite=Lax"] {
        assert!(cookie.contains(attribute), "{attribute} missing: {cookie}");
    }
}

#[tokio::test]
async fn unknown_auth_route_is_not_the_app() {
    let app = App::new(None).await;
    app.seed_user().await;
    let reply = app.get("/api/auth/nope", None).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}
