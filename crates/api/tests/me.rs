use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{role_permissions, roles, user_permissions, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";

struct App {
    router: Router,
    state: AppState,
    _dir: TempDir,
}

impl App {
    /// A database past setup, with its Owner.
    async fn new() -> (Self, String) {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        let app = Self {
            router: router(state.clone()),
            state,
            _dir: dir,
        };
        let body = json!({ "name": "Owner", "email": "owner@example.com", "password": PASSWORD });
        let (status, cookie, reply) = app.post("/api/v1/setup", body).await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        (app, cookie.unwrap())
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Option<String>, Value) {
        let response = self.router.clone().oneshot(request).await.unwrap();
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

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Option<String>, Value) {
        let request = Request::post(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    async fn me(&self, cookie: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::get("/api/v1/me").header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let (status, _, body) = self.send(request.body(Body::empty()).unwrap()).await;
        (status, body)
    }

    /// A second user, created the way the users API will: one admitted sign-up.
    async fn user(&self, email: &str) -> (String, String) {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let (status, cookie, reply) = self.post("/api/auth/sign-up/email", body).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let id = reply["user"]["id"].as_str().unwrap().to_owned();
        (id, cookie.unwrap())
    }
}

#[tokio::test]
async fn owner_sees_the_whole_catalog() {
    let (app, cookie) = App::new().await;
    let (status, body) = app.me(Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], "owner@example.com");
    assert_eq!(body["isOwner"], true);
    assert_eq!(body["roles"][0]["name"], "Owner");
    assert_eq!(body["roles"][0]["isOwner"], true);
    let permissions = body["permissions"].as_array().unwrap();
    assert_eq!(
        permissions.len(),
        unseen_core::permission::Permission::ALL.len()
    );
    assert_eq!(permissions[0], "library:read");
}

#[tokio::test]
async fn user_without_a_role_has_no_permission() {
    let (app, _) = App::new().await;
    let (_, cookie) = app.user("member@example.com").await;
    let (status, body) = app.me(Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["isOwner"], false);
    assert_eq!(body["roles"], json!([]));
    assert_eq!(body["permissions"], json!([]));
}

#[tokio::test]
async fn roles_and_direct_grants_are_united_in_catalog_order() {
    let (app, _) = App::new().await;
    let (id, cookie) = app.user("member@example.com").await;
    let role = roles::ActiveModel {
        name: Set("Searcher".into()),
        is_owner: Set(false),
        created_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    for permission in ["search:run", "library:read", "not:a-permission"] {
        role_permissions::ActiveModel {
            role_id: Set(role.id),
            permission: Set(permission.into()),
        }
        .insert(&app.state.db)
        .await
        .unwrap();
    }
    user_roles::ActiveModel {
        user_id: Set(id.clone()),
        role_id: Set(role.id),
        all_libraries: Set(true),
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    user_permissions::ActiveModel {
        user_id: Set(id),
        permission: Set("queue:read".into()),
        all_libraries: Set(true),
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let (_, body) = app.me(Some(&cookie)).await;
    assert_eq!(body["roles"][0]["name"], "Searcher");
    // Unknown strings are dropped; the order is the catalog's.
    assert_eq!(
        body["permissions"],
        json!(["library:read", "search:run", "queue:read"])
    );
}

#[tokio::test]
async fn signed_out_is_401() {
    let (app, _) = App::new().await;
    let (status, body) = app.me(None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized");
}

#[test]
fn openapi_has_the_permission_enum() {
    let document = serde_json::to_value(api::openapi()).unwrap();
    let operation = &document["paths"]["/api/v1/me"]["get"];
    assert_eq!(operation["x-permission"], "authenticated");
    let schema = &document["components"]["schemas"]["Permission"];
    let values = schema["enum"].as_array().expect("Permission is an enum");
    assert_eq!(values.len(), unseen_core::permission::Permission::ALL.len());
}
