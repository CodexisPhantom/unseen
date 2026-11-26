use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
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

fn actions(log: &Value) -> Vec<&str> {
    log["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["action"].as_str().unwrap())
        .collect()
}

impl App {
    async fn log(&self, query: &str) -> Value {
        let (status, body) = self
            .owner(Method::GET, &format!("/api/v1/audit-log{query}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

#[tokio::test]
async fn setup_and_the_sign_in_events_are_recorded() {
    let app = App::new().await;
    let sign_in = json!({ "email": "owner@example.com", "password": PASSWORD });
    let (status, cookie, _) = app
        .send(Method::POST, "/api/auth/sign-in/email", None, Some(sign_in))
        .await;
    assert_eq!(status, StatusCode::OK);
    let wrong = json!({ "email": "owner@example.com", "password": "not the password" });
    let (status, _, _) = app
        .send(Method::POST, "/api/auth/sign-in/email", None, Some(wrong))
        .await;
    assert!(status.is_client_error());
    let (status, _, _) = app
        .send(
            Method::POST,
            "/api/auth/sign-out",
            cookie.as_deref(),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let log = app.log("").await;
    assert_eq!(
        actions(&log),
        [
            "auth.sign_out",
            "auth.sign_in_failed",
            "auth.sign_in",
            "setup.completed"
        ]
    );
    let items = log["items"].as_array().unwrap();
    assert_eq!(items[0]["actorName"], "Owner");
    assert_eq!(items[1]["actorName"], "owner@example.com");
    assert_eq!(items[1]["actorUserId"], Value::Null);
    assert_eq!(items[3]["targetKind"], "user");
    assert_eq!(items[3]["targetId"], items[2]["targetId"]);
}

#[tokio::test]
async fn changing_a_role_writes_one_row_with_the_old_and_new_permissions() {
    let app = App::new().await;
    let (status, role) = app
        .owner(
            Method::POST,
            "/api/v1/roles",
            Some(json!({ "name": "Reader", "permissions": ["library:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    let id = role["id"].as_i64().unwrap();
    let patch = json!({ "permissions": ["library:read", "queue:read"] });
    let (status, _) = app
        .owner(Method::PATCH, &format!("/api/v1/roles/{id}"), Some(patch))
        .await;
    assert_eq!(status, StatusCode::OK);

    let log = app.log("?action=role.updated").await;
    assert_eq!(log["total"], 1);
    let row = &log["items"][0];
    assert_eq!(row["targetKind"], "role");
    assert_eq!(row["targetId"], id.to_string());
    assert_eq!(row["actorName"], "Owner");
    assert_eq!(
        row["data"],
        json!({ "permissions": {
            "old": ["library:read"],
            "new": ["library:read", "queue:read"],
        } })
    );

    let (status, _) = app
        .owner(Method::DELETE, &format!("/api/v1/roles/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let log = app.log("?targetKind=role").await;
    assert_eq!(
        actions(&log),
        ["role.deleted", "role.updated", "role.created"]
    );
}

#[tokio::test]
async fn a_secret_never_reaches_the_log() {
    let app = App::new().await;
    let (status, user) = app
        .owner(
            Method::POST,
            "/api/v1/users",
            Some(
                json!({ "name": "Ann", "email": "ann@example.com", "password": "first secret pw" }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{user}");
    let id = user["id"].as_str().unwrap();
    let (status, _) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(json!({ "password": "second secret pw", "name": "Anne" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, key) = app
        .owner(
            Method::POST,
            "/api/v1/me/api-keys",
            Some(json!({ "name": "tool", "permissions": ["library:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");

    let log = app.log("").await;
    let text = log.to_string();
    for secret in [
        "first secret pw",
        "second secret pw",
        key["key"].as_str().unwrap(),
    ] {
        assert!(!text.contains(secret), "the log holds a secret");
    }
    let updated = app.log("?action=user.updated").await;
    assert_eq!(
        updated["items"][0]["data"],
        json!({ "password": "changed", "name": { "old": "Ann", "new": "Anne" } })
    );
    assert_eq!(
        actions(&app.log("?action=api_key.created").await),
        ["api_key.created"]
    );
}

#[tokio::test]
async fn a_row_stays_readable_after_its_actor_is_deleted() {
    let app = App::new().await;
    let (user_id, cookie, _) = app.member("m@example.com", &["roles:manage"]).await;
    let (status, _, _) = app
        .send(
            Method::POST,
            "/api/v1/roles",
            Some(&cookie),
            Some(json!({ "name": "Made by Member", "permissions": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app
        .owner(Method::DELETE, &format!("/api/v1/users/{user_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let log = app.log("?action=role.created").await;
    assert_eq!(log["total"], 1);
    assert_eq!(log["items"][0]["actorName"], "Member");
    assert_eq!(log["items"][0]["actorUserId"], Value::Null);
}

#[tokio::test]
async fn filters_and_pages_the_log() {
    let app = App::new().await;
    for name in ["A", "B", "C"] {
        app.owner(
            Method::POST,
            "/api/v1/roles",
            Some(json!({ "name": name, "permissions": [] })),
        )
        .await;
    }
    let page = app.log("?action=role.created&pageSize=2&page=2").await;
    assert_eq!(page["total"], 3);
    assert_eq!(page["page"], 2);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);

    let from = app.log("?from=2999-01-01T00:00:00Z").await;
    assert_eq!(from["total"], 0);
    let to = app.log("?to=2999-01-01T00:00:00Z").await;
    assert_eq!(to["total"], 4);
    let owner = app.log("").await["items"][0]["actorUserId"].clone();
    let by_actor = app
        .log(&format!("?actor={}", owner.as_str().unwrap()))
        .await;
    assert_eq!(by_actor["total"], 4);

    for query in ["?from=yesterday", "?pageSize=501", "?page=0"] {
        let (status, _) = app
            .owner(Method::GET, &format!("/api/v1/audit-log{query}"), None)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
    }
}

#[tokio::test]
async fn needs_system_audit() {
    let app = App::new().await;
    let (_, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    let (status, _, body) = app
        .send(Method::GET, "/api/v1/audit-log", Some(&cookie), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["requiredPermission"], "system:audit");
    let (_, cookie, _) = app.member("a@example.com", &["system:audit"]).await;
    let (status, _, _) = app
        .send(Method::GET, "/api/v1/audit-log", Some(&cookie), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}
