use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";

struct App {
    router: Router,
    _dir: TempDir,
}

impl App {
    async fn new(base_url: Option<&str>, seed: bool) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        if seed {
            db::testing::insert_user(&db, "u1").await;
        }
        let state = AppState::new(db, "test secret", base_url).await.unwrap();
        Self {
            router: router(state),
            _dir: dir,
        }
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let request = Request::get(path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }
}

#[tokio::test]
async fn api_routes_are_served_only_under_the_base_path() {
    let app = App::new(Some("http://unseen.test/unseen"), true).await;
    let (status, body) = app.get("/unseen/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));

    let (status, _) = app.get("/api/v1/health").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get("/unseenfoo/api/v1/health").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_routes_are_served_under_the_base_path() {
    let app = App::new(Some("http://unseen.test/unseen"), true).await;
    let (status, body) = app.get("/unseen/api/auth/ok").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let (status, _) = app.get("/api/auth/ok").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn setup_works_under_the_base_path() {
    let app = App::new(Some("http://unseen.test/unseen"), false).await;
    let body = json!({"name":"Owner","email":"owner@example.com","password":"correct horse"});
    let request = Request::post("/unseen/api/v1/setup")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "http://unseen.test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    let cookie = cookies
        .iter()
        .find(|c| c.contains("session_token="))
        .expect("a session cookie");
    assert!(cookie.contains("Path=/"));
}

#[tokio::test]
async fn root_base_keeps_the_existing_paths() {
    let (db, _dir) = db::testing::temp_db().await;
    db::testing::insert_user(&db, "u1").await;
    let state = AppState::new(db, "test secret", None).await.unwrap();
    assert!(state.base_path.is_empty());
    let router = router(state);
    let response = router
        .oneshot(
            Request::get("/api/v1/health")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}
