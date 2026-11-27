//! API keys: `X-Api-Key` and `?apikey=` authenticate, and a key never exceeds
//! its owner (docs/15-permissions.md, "API keys").

use api::access::{ApiRouter, LibraryEdit, LibraryRead, Require};
use api::{AppState, routes};
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::{Json, Router};
use better_auth::integrations::axum::AxumIntegration;
use better_auth::seaorm::sea_orm::{
    ActiveModelTrait, ConnectionTrait, EntityTrait, Set, Statement,
};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use unseenarr_core::permission::Permission;
use utoipa_axum::routes;

const HOST: &str = "unseenarr.test";
const ORIGIN: &str = "http://unseenarr.test";
const PASSWORD: &str = "correct horse";

/// Stand-ins for the library routes of M2.
#[utoipa::path(get, path = "/api/v1/test/read", responses((status = 200)))]
async fn read(_: Require<LibraryRead>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

#[utoipa::path(post, path = "/api/v1/test/write", responses((status = 200)))]
async fn write(_: Require<LibraryEdit>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

/// How a request proves who sends it.
enum As<'a> {
    Cookie(&'a str),
    Header(&'a str),
    Nobody,
}

impl App {
    async fn new() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        let extra = ApiRouter::new()
            .require(Permission::LibraryRead, routes!(read))
            .require(Permission::LibraryEdit, routes!(write));
        let router = routes()
            .merge(extra)
            .into_router(&state)
            .nest("/api/auth", state.auth.clone().axum_router_with_state())
            .with_state(state.clone());
        let mut app = Self {
            router,
            state,
            owner: String::new(),
            _dir: dir,
        };
        let body = json!({ "name": "Owner", "email": "owner@example.com", "password": PASSWORD });
        let (status, cookie, reply) = app
            .send(Method::POST, "/api/v1/setup", As::Nobody, Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        app.owner = cookie.unwrap();
        app
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        who: As<'_>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST);
        match who {
            // A browser session sends its origin. A key sends neither.
            As::Cookie(cookie) => {
                request = request
                    .header(header::COOKIE, cookie)
                    .header(header::ORIGIN, ORIGIN);
            }
            As::Header(key) => request = request.header("X-Api-Key", key),
            As::Nobody => request = request.header(header::ORIGIN, ORIGIN),
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

    async fn status(&self, method: Method, path: &str, who: As<'_>) -> StatusCode {
        self.send(method, path, who, None).await.0
    }

    /// A user holding a new role with `permissions`: id, cookie and role id.
    async fn member(&self, email: &str, permissions: &[&str]) -> (String, String, i32) {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let (status, cookie, reply) = self
            .send(
                Method::POST,
                "/api/auth/sign-up/email",
                As::Nobody,
                Some(body),
            )
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

    /// Creates a key signed in. Returns its value and its id.
    async fn key(&self, cookie: &str, permissions: &[&str]) -> (String, i64) {
        let body = json!({ "name": "Seerr", "permissions": permissions });
        let (status, _, reply) = self
            .send(
                Method::POST,
                "/api/v1/me/api-keys",
                As::Cookie(cookie),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        (
            reply["key"].as_str().unwrap().to_owned(),
            reply["apiKey"]["id"].as_i64().unwrap(),
        )
    }
}

#[tokio::test]
async fn a_key_reads_with_either_form_and_is_refused_a_write() {
    let app = App::new().await;
    let (_, cookie, _) = app
        .member("m@example.com", &["library:read", "library:edit"])
        .await;
    let (key, _) = app.key(&cookie, &["library:read"]).await;

    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::OK
    );
    let by_query = format!("/api/v1/test/read?apikey={key}");
    assert_eq!(
        app.status(Method::GET, &by_query, As::Nobody).await,
        StatusCode::OK
    );
    assert_eq!(
        app.status(Method::POST, "/api/v1/test/write", As::Header(&key))
            .await,
        StatusCode::FORBIDDEN
    );
    // The owner of the key can still write: the key is the narrower one.
    assert_eq!(
        app.status(Method::POST, "/api/v1/test/write", As::Cookie(&cookie))
            .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn removing_a_role_from_the_owner_narrows_the_key_at_once() {
    let app = App::new().await;
    let (user, cookie, role) = app.member("m@example.com", &["library:read"]).await;
    let (key, _) = app.key(&cookie, &["library:read"]).await;
    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::OK
    );

    user_roles::Entity::delete_by_id((user, role))
        .exec(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn the_key_is_stored_hashed_and_never_listed() {
    let app = App::new().await;
    let (key, _) = app.key(&app.owner, &["library:read"]).await;

    let row = app
        .state
        .db
        .query_one_raw(Statement::from_string(
            app.state.db.get_database_backend(),
            "SELECT key_hash, key_prefix FROM api_keys",
        ))
        .await
        .unwrap()
        .unwrap();
    let hash: String = row.try_get("", "key_hash").unwrap();
    assert_ne!(hash, key);
    assert_eq!(hash.len(), 64);
    assert_eq!(row.try_get::<String>("", "key_prefix").unwrap(), key[..8]);

    let (status, _, listed) = app
        .send(
            Method::GET,
            "/api/v1/me/api-keys",
            As::Cookie(&app.owner),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!listed.to_string().contains(&key));
    assert_eq!(listed[0]["prefix"], key[..8]);
    assert_eq!(listed[0]["permissions"], json!(["library:read"]));
}

#[tokio::test]
async fn a_bad_revoked_or_disabled_key_is_refused_and_never_falls_back_to_the_cookie() {
    let app = App::new().await;
    let (user, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    let (key, id) = app.key(&cookie, &["library:read"]).await;

    // A valid session beside an unknown key still fails.
    let request = Request::get("/api/v1/test/read")
        .header(header::HOST, HOST)
        .header(header::COOKIE, &app.owner)
        .header("X-Api-Key", "nope")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let disable = json!({ "disabled": true });
    let (status, _, reply) = app
        .send(
            Method::PATCH,
            &format!("/api/v1/users/{user}"),
            As::Cookie(&app.owner),
            Some(disable),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::UNAUTHORIZED
    );
    let enable = json!({ "disabled": false });
    app.send(
        Method::PATCH,
        &format!("/api/v1/users/{user}"),
        As::Cookie(&app.owner),
        Some(enable),
    )
    .await;
    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::OK
    );

    let path = format!("/api/v1/me/api-keys/{id}");
    // A key can revoke itself, and then it is gone.
    assert_eq!(
        app.status(Method::DELETE, &path, As::Header(&key)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_key_never_holds_the_owner_role_or_creates_keys() {
    let app = App::new().await;
    let (key, _) = app.key(&app.owner, &["library:read"]).await;
    let (_, _, me) = app
        .send(Method::GET, "/api/v1/me", As::Header(&key), None)
        .await;
    assert_eq!(me["isOwner"], false);
    assert_eq!(me["permissions"], json!(["library:read"]));

    let body = json!({ "name": "Copy", "permissions": ["library:read"] });
    let (status, _, _) = app
        .send(
            Method::POST,
            "/api/v1/me/api-keys",
            As::Header(&key),
            Some(body),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_key_cannot_carry_a_permission_its_creator_lacks() {
    let app = App::new().await;
    let (_, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    let body = json!({ "name": "Too much", "permissions": ["library:read", "users:manage"] });
    let (status, _, reply) = app
        .send(
            Method::POST,
            "/api/v1/me/api-keys",
            As::Cookie(&cookie),
            Some(body),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    let body = json!({ "name": "Typo", "permissions": ["library:fly"] });
    let (status, _, _) = app
        .send(
            Method::POST,
            "/api/v1/me/api-keys",
            As::Cookie(&cookie),
            Some(body),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn other_users_keys_need_apikeys_manage() {
    let app = App::new().await;
    let (_, member, _) = app.member("m@example.com", &["library:read"]).await;
    let (_, admin, _) = app.member("a@example.com", &["apikeys:manage"]).await;
    let (key, id) = app.key(&member, &["library:read"]).await;

    assert_eq!(
        app.status(Method::GET, "/api/v1/api-keys", As::Cookie(&member))
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.status(
            Method::DELETE,
            &format!("/api/v1/api-keys/{id}"),
            As::Cookie(&member)
        )
        .await,
        StatusCode::FORBIDDEN
    );
    // Through the own-keys route another user's key does not exist.
    let path = format!("/api/v1/me/api-keys/{id}");
    assert_eq!(
        app.status(Method::DELETE, &path, As::Cookie(&admin)).await,
        StatusCode::NOT_FOUND
    );

    let (status, _, all) = app
        .send(Method::GET, "/api/v1/api-keys", As::Cookie(&admin), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(all[0]["userName"], "Member");
    let revoke = format!("/api/v1/api-keys/{id}");
    assert_eq!(
        app.status(Method::DELETE, &revoke, As::Cookie(&admin))
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.status(Method::DELETE, &revoke, As::Cookie(&admin))
            .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.status(Method::GET, "/api/v1/test/read", As::Header(&key))
            .await,
        StatusCode::UNAUTHORIZED
    );
}
