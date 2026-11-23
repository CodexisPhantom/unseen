use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use db::auth_schema::user;
use db::entities::roles;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";

struct App {
    router: Router,
    state: AppState,
    _dir: TempDir,
}

struct Reply {
    status: StatusCode,
    cookies: Vec<String>,
    body: Value,
}

impl App {
    async fn new() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        Self {
            router: router(state.clone()),
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

    async fn get(&self, path: &str, cookie: Option<&str>) -> Reply {
        let mut request = Request::get(path).header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    async fn setup(&self, email: &str, password: &str) -> Reply {
        let body = json!({ "name": "Owner", "email": email, "password": password });
        let request = Request::post("/api/v1/setup")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, "http://unseen.test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    async fn users(&self) -> u64 {
        user::Entity::find().count(&self.state.db).await.unwrap()
    }
}

#[tokio::test]
async fn setup_creates_a_signed_in_owner_once() {
    let app = App::new().await;
    assert_eq!(app.get("/api/v1/setup", None).await.body["required"], true);

    let reply = app.setup("owner@example.com", "correct horse").await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.body["email"], "owner@example.com");
    let user_id = reply.body["id"].as_str().unwrap();
    let grants = db::access::grants_of_user(&app.state.db, user_id)
        .await
        .unwrap();
    assert!(grants.is_owner);

    // The response signs the Owner in.
    let cookie = reply
        .cookies
        .iter()
        .find(|cookie| cookie.contains("session_token="))
        .expect("a session cookie");
    let pair = cookie.split(';').next().unwrap();
    let session = app.get("/api/auth/get-session", Some(pair)).await;
    assert_eq!(session.body["user"]["id"], user_id);

    assert_eq!(app.get("/api/v1/setup", None).await.body["required"], false);
    let again = app.setup("second@example.com", "correct horse").await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.body["code"], "setup_done");
    assert_eq!(app.users().await, 1);
}

#[tokio::test]
async fn two_parallel_setups_create_exactly_one_user() {
    let app = App::new().await;
    let (first, second) = tokio::join!(
        app.setup("first@example.com", "correct horse"),
        app.setup("second@example.com", "correct horse"),
    );
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
    assert_eq!(app.users().await, 1);
}

#[tokio::test]
async fn api_answers_setup_required_until_the_owner_exists() {
    let app = App::new().await;
    let reply = app.get("/api/v1/nope", None).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["code"], "setup_required");
    // The auth routes too: no sign-up or sign-in before setup.
    let reply = app.get("/api/auth/get-session", None).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    // Health and static files stay served.
    assert_eq!(app.get("/api/v1/health", None).await.status, StatusCode::OK);
    assert_ne!(
        app.get("/setup", None).await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );

    app.setup("owner@example.com", "correct horse").await;
    assert_eq!(
        app.get("/api/v1/nope", None).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn refused_password_creates_no_user_and_keeps_setup_open() {
    let app = App::new().await;
    let reply = app.setup("owner@example.com", "123456789").await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        reply.body
    );
    assert_eq!(reply.body["code"], "validation");
    assert!(reply.body["detail"].is_string(), "{}", reply.body);
    assert_eq!(app.users().await, 0);

    let reply = app.setup("owner@example.com", "1234567890").await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
}

#[tokio::test]
async fn user_and_owner_role_commit_together() {
    let app = App::new().await;
    // Without an Owner role the assignment fails; the user must not stay.
    roles::Entity::delete_many()
        .filter(roles::Column::IsOwner.eq(true))
        .exec(&app.state.db)
        .await
        .unwrap();
    let reply = app.setup("owner@example.com", "correct horse").await;
    assert_eq!(
        reply.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        reply.body
    );
    assert_eq!(app.users().await, 0);
    assert_eq!(app.get("/api/v1/setup", None).await.body["required"], true);
}
