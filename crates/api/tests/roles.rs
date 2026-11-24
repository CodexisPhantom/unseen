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
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        (user_id, cookie.unwrap(), role.id)
    }
}

#[tokio::test]
async fn lists_the_catalog_and_the_roles_with_the_owner_first() {
    let app = App::new().await;
    let (status, catalog) = app.owner(Method::GET, "/api/v1/permissions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        catalog.as_array().unwrap().len(),
        unseen_core::permission::Permission::ALL.len()
    );

    let (status, roles) = app.owner(Method::GET, "/api/v1/roles", None).await;
    assert_eq!(status, StatusCode::OK, "{roles}");
    assert_eq!(roles[0]["name"], "Owner");
    assert_eq!(roles[0]["isOwner"], true);
    assert_eq!(roles[0]["users"], 1);
    assert_eq!(roles[0]["permissions"], catalog);
}

#[tokio::test]
async fn needs_roles_manage() {
    let app = App::new().await;
    let (_, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    for (method, path) in [
        (Method::GET, "/api/v1/roles"),
        (Method::DELETE, "/api/v1/roles/1"),
    ] {
        let (status, _, body) = app.send(method, path, Some(&cookie), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(body["requiredPermission"], "roles:manage");
    }
    let (status, _, _) = app.send(Method::GET, "/api/v1/roles", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_catalog_is_open_to_any_user_and_roles_to_users_manage() {
    let app = App::new().await;
    let (_, reader, _) = app.member("r@example.com", &["library:read"]).await;
    let (status, _, catalog) = app
        .send(Method::GET, "/api/v1/permissions", Some(&reader), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(catalog.as_array().unwrap().len() > 20);
    let (status, _, _) = app
        .send(Method::GET, "/api/v1/permissions", None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _, body) = app
        .send(Method::GET, "/api/v1/assignable-roles", Some(&reader), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["requiredPermission"], "users:manage");
    let (_, admin, _) = app.member("a@example.com", &["users:manage"]).await;
    let (status, _, roles) = app
        .send(Method::GET, "/api/v1/assignable-roles", Some(&admin), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{roles}");
    assert_eq!(roles[0]["name"], "Owner");
}

#[tokio::test]
async fn creates_updates_and_deletes_a_role() {
    let app = App::new().await;
    let body = json!({
        "name": "  Member ",
        "description": "Family",
        "permissions": ["library:read", "queue:read"]
    });
    let (status, role) = app.owner(Method::POST, "/api/v1/roles", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    assert_eq!(role["name"], "Member");
    assert_eq!(role["permissions"], json!(["library:read", "queue:read"]));
    let path = format!("/api/v1/roles/{}", role["id"]);

    let patch = json!({ "name": "Viewer", "description": "", "permissions": ["queue:read"] });
    let (status, role) = app.owner(Method::PATCH, &path, Some(patch)).await;
    assert_eq!(status, StatusCode::OK, "{role}");
    assert_eq!(role["name"], "Viewer");
    assert_eq!(role["description"], Value::Null);
    assert_eq!(role["permissions"], json!(["queue:read"]));

    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn validates_the_input() {
    let app = App::new().await;
    for (body, field) in [
        (json!({ "name": "  ", "permissions": [] }), "name"),
        (json!({ "name": "x".repeat(65), "permissions": [] }), "name"),
        (
            json!({ "name": "A", "permissions": ["library:fly"] }),
            "permissions",
        ),
    ] {
        let (status, reply) = app.owner(Method::POST, "/api/v1/roles", Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "validation");
        assert_eq!(reply["errors"][0]["field"], field);
    }

    let body = json!({ "name": "A", "permissions": [] });
    let (status, _) = app
        .owner(Method::POST, "/api/v1/roles", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, reply) = app.owner(Method::POST, "/api/v1/roles", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "role_name_taken");
    let owner_name = json!({ "name": "Owner", "permissions": [] });
    let (status, _) = app
        .owner(Method::POST, "/api/v1/roles", Some(owner_name))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_role_cannot_gain_a_permission_its_editor_lacks() {
    let app = App::new().await;
    let (_, cookie, own_role) = app
        .member("m@example.com", &["roles:manage", "library:read"])
        .await;

    let body = json!({ "name": "Deleters", "permissions": ["library:read", "library:delete"] });
    let (status, _, reply) = app
        .send(Method::POST, "/api/v1/roles", Some(&cookie), Some(body))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["requiredPermission"], "library:delete");

    let body = json!({ "name": "Readers", "permissions": ["library:read"] });
    let (status, _, reply) = app
        .send(Method::POST, "/api/v1/roles", Some(&cookie), Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    let path = format!("/api/v1/roles/{}", reply["id"]);
    let patch = json!({ "permissions": ["library:read", "system:backup"] });
    let (status, _, reply) = app
        .send(Method::PATCH, &path, Some(&cookie), Some(patch))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");

    // A role that holds a permission the editor lacks cannot be edited or deleted.
    let body = json!({ "name": "Backups", "permissions": ["system:backup"] });
    let (_, backups) = app.owner(Method::POST, "/api/v1/roles", Some(body)).await;
    let path = format!("/api/v1/roles/{}", backups["id"]);
    let (status, _, _) = app
        .send(
            Method::PATCH,
            &path,
            Some(&cookie),
            Some(json!({ "name": "B" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = app.send(Method::DELETE, &path, Some(&cookie), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_ne!(own_role, 0);
}

#[tokio::test]
async fn the_owner_role_is_read_only_even_for_the_owner() {
    let app = App::new().await;
    let (_, roles) = app.owner(Method::GET, "/api/v1/roles", None).await;
    let path = format!("/api/v1/roles/{}", roles[0]["id"]);
    let (status, reply) = app
        .owner(Method::PATCH, &path, Some(json!({ "name": "Boss" })))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["code"], "owner_role_readonly");
    let (status, reply) = app
        .owner(Method::DELETE, &format!("{path}?confirm=true"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["code"], "owner_role_readonly");
}

#[tokio::test]
async fn deleting_a_role_in_use_needs_confirmation() {
    let app = App::new().await;
    let (_, cookie, role) = app.member("m@example.com", &["library:read"]).await;
    let path = format!("/api/v1/roles/{role}");

    let (status, reply) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "role_in_use");
    let (status, _) = app
        .owner(Method::DELETE, &format!("{path}?confirm=false"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = app
        .owner(Method::DELETE, &format!("{path}?confirm=true"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // The user lost the role at once.
    let (_, _, me) = app
        .send(Method::GET, "/api/v1/me", Some(&cookie), None)
        .await;
    assert_eq!(me["permissions"], json!([]));
}

#[tokio::test]
async fn a_changed_role_applies_on_the_next_request() {
    let app = App::new().await;
    let (_, cookie, role) = app.member("m@example.com", &["library:read"]).await;
    let (_, _, me) = app
        .send(Method::GET, "/api/v1/me", Some(&cookie), None)
        .await;
    assert_eq!(me["permissions"], json!(["library:read"]));

    let patch = json!({ "permissions": ["library:read", "queue:read"] });
    let (status, _) = app
        .owner(Method::PATCH, &format!("/api/v1/roles/{role}"), Some(patch))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, me) = app
        .send(Method::GET, "/api/v1/me", Some(&cookie), None)
        .await;
    assert_eq!(me["permissions"], json!(["library:read", "queue:read"]));
}
