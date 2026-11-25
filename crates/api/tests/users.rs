use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use db::users;
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

impl App {
    async fn owner_role(&self) -> i64 {
        db::roles::list(&self.state.db).await.unwrap()[0]
            .role
            .id
            .into()
    }

    async fn sign_in(&self, email: &str, password: &str) -> (StatusCode, Option<String>, Value) {
        let body = json!({ "email": email, "password": password });
        self.send(Method::POST, "/api/auth/sign-in/email", None, Some(body))
            .await
    }

    async fn me(&self, cookie: &str) -> StatusCode {
        self.send(Method::GET, "/api/v1/me", Some(cookie), None)
            .await
            .0
    }

    async fn owner_id(&self) -> String {
        let (_, users) = self.owner(Method::GET, "/api/v1/users", None).await;
        users[0]["id"].as_str().unwrap().to_owned()
    }
}

#[tokio::test]
async fn a_created_user_signs_in_with_the_given_password() {
    let app = App::new().await;
    let role = db::roles::create(
        &app.state.db,
        "Member",
        None,
        &["library:read".to_owned()].into(),
    )
    .await
    .unwrap()
    .role
    .id;
    let body = json!({
        "name": "Ann", "email": "Ann@Example.com", "password": "a long password", "roleIds": [role]
    });
    let (status, user) = app
        .owner(Method::POST, "/api/v1/users", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{user}");
    assert_eq!(user["email"], "ann@example.com");
    assert_eq!(user["roles"][0]["name"], "Member");
    assert_eq!(user["disabled"], false);

    let (status, cookie, _) = app.sign_in("ann@example.com", "a long password").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.me(&cookie.unwrap()).await, StatusCode::OK);
    // The creation itself left no session behind.
    let sessions = db::auth_schema::session::Entity::find()
        .all(&app.state.db)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 2, "the Owner and the sign-in above");

    let (status, reply) = app.owner(Method::POST, "/api/v1/users", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "email_taken");
}

#[tokio::test]
async fn validates_the_input() {
    let app = App::new().await;
    for (body, field) in [
        (
            json!({ "name": " ", "email": "a@b.co", "password": "a long password" }),
            "name",
        ),
        (
            json!({ "name": "A", "email": "nope", "password": "a long password" }),
            "email",
        ),
        (
            json!({ "name": "A", "email": "a@b.co", "password": "short" }),
            "password",
        ),
        (
            json!({ "name": "A", "email": "a@b.co", "password": "a long password", "roleIds": [999] }),
            "roleIds",
        ),
    ] {
        let (status, reply) = app.owner(Method::POST, "/api/v1/users", Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["errors"][0]["field"], field);
    }
}

#[tokio::test]
async fn disabling_ends_the_session_and_blocks_sign_in_until_enabled() {
    let app = App::new().await;
    let (id, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    assert_eq!(app.me(&cookie).await, StatusCode::OK);

    let path = format!("/api/v1/users/{id}");
    let (status, user) = app
        .owner(Method::PATCH, &path, Some(json!({ "disabled": true })))
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    assert_eq!(user["disabled"], true);
    assert_eq!(app.me(&cookie).await, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.sign_in("m@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = app
        .owner(Method::PATCH, &path, Some(json!({ "disabled": false })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app.sign_in("m@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_restart_keeps_users_disabled() {
    let app = App::new().await;
    let (id, _, _) = app.member("m@example.com", &[]).await;
    let path = format!("/api/v1/users/{id}");
    app.owner(Method::PATCH, &path, Some(json!({ "disabled": true })))
        .await;

    let state = AppState::new(app.state.db.clone(), "test secret", None)
        .await
        .unwrap();
    let restarted = App {
        router: router(state.clone()),
        state,
        owner: app.owner.clone(),
        _dir: tempfile::tempdir().unwrap(),
    };
    let (status, _, _) = restarted.sign_in("m@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_last_owner_cannot_be_disabled_deleted_or_demoted() {
    let app = App::new().await;
    let owner = app.owner_id().await;
    let path = format!("/api/v1/users/{owner}");
    for patch in [json!({ "disabled": true }), json!({ "roleIds": [] })] {
        let (status, reply) = app.owner(Method::PATCH, &path, Some(patch)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "last_owner");
    }
    let (status, reply) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "last_owner");

    // With a second Owner the first can go.
    let (second, _, _) = app.member("two@example.com", &[]).await;
    let role = app.owner_role().await;
    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{second}"),
            Some(json!({ "roleIds": [role] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["isOwner"], true);
    let (status, _) = app
        .owner(Method::PATCH, &path, Some(json!({ "disabled": true })))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn only_an_owner_changes_an_owner() {
    let app = App::new().await;
    let owner = app.owner_id().await;
    let (_, cookie, _) = app
        .member("admin@example.com", &["users:manage", "roles:manage"])
        .await;
    let path = format!("/api/v1/users/{owner}");
    let calls = [
        (Method::PATCH, path.clone(), Some(json!({ "name": "Boss" }))),
        (
            Method::PATCH,
            path.clone(),
            Some(json!({ "password": "another long one" })),
        ),
        (Method::DELETE, path.clone(), None),
        (
            Method::PUT,
            format!("{path}/permissions"),
            Some(json!({ "permissions": [] })),
        ),
    ];
    for (method, path, body) in calls {
        let (status, _, reply) = app.send(method, &path, Some(&cookie), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} {reply}");
        assert_eq!(reply["code"], "owner_only");
    }

    // Not even an admin who holds every permission may hand out the Owner role.
    let (target, _, _) = app.member("t@example.com", &[]).await;
    let role = app.owner_role().await;
    let (status, _, reply) = app
        .send(
            Method::PATCH,
            &format!("/api/v1/users/{target}"),
            Some(&cookie),
            Some(json!({ "roleIds": [role] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["code"], "owner_only");
}

#[tokio::test]
async fn roles_and_grants_beyond_the_editors_own_are_refused() {
    let app = App::new().await;
    let (_, cookie, _) = app
        .member("admin@example.com", &["users:manage", "library:read"])
        .await;
    let (target, _, _) = app.member("t@example.com", &[]).await;
    let path = format!("/api/v1/users/{target}");

    let backups = db::roles::create(
        &app.state.db,
        "Backups",
        None,
        &["system:backup".to_owned()].into(),
    )
    .await
    .unwrap()
    .role
    .id;
    let readers = db::roles::create(
        &app.state.db,
        "Readers",
        None,
        &["library:read".to_owned()].into(),
    )
    .await
    .unwrap()
    .role
    .id;
    let (status, _, reply) = app
        .send(
            Method::PATCH,
            &path,
            Some(&cookie),
            Some(json!({ "roleIds": [backups] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["requiredPermission"], "system:backup");
    let (status, _, _) = app
        .send(
            Method::PATCH,
            &path,
            Some(&cookie),
            Some(json!({ "roleIds": [readers] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let grants = format!("{path}/permissions");
    let (status, _, reply) = app
        .send(
            Method::PUT,
            &grants,
            Some(&cookie),
            Some(json!({ "permissions": ["queue:manage"] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["requiredPermission"], "queue:manage");
    let (status, _, user) = app
        .send(
            Method::PUT,
            &grants,
            Some(&cookie),
            Some(json!({ "permissions": ["library:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    assert_eq!(user["permissions"], json!(["library:read"]));

    // The Owner may grant anything, and the grant shows in /me.
    let (status, _) = app
        .owner(
            Method::PUT,
            &grants,
            Some(json!({ "permissions": ["queue:manage", "library:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // A grant the editor lacks cannot be taken away either.
    let (status, _, _) = app
        .send(
            Method::PUT,
            &grants,
            Some(&cookie),
            Some(json!({ "permissions": ["library:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = app
        .owner(
            Method::PUT,
            &grants,
            Some(json!({ "permissions": ["nope:nope"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_new_password_replaces_the_old_one_and_ends_sessions() {
    let app = App::new().await;
    let (id, cookie, _) = app.member("m@example.com", &[]).await;
    let path = format!("/api/v1/users/{id}");
    let (status, _) = app
        .owner(Method::PATCH, &path, Some(json!({ "password": "short" })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "password": "another long one" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(app.me(&cookie).await, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.sign_in("m@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.sign_in("m@example.com", "another long one").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn deleting_a_user_removes_them_and_their_sessions() {
    let app = App::new().await;
    let (id, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    let path = format!("/api/v1/users/{id}");
    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.me(&cookie).await, StatusCode::UNAUTHORIZED);
    assert!(users::find(&app.state.db, &id).await.unwrap().is_none());
    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn needs_users_manage() {
    let app = App::new().await;
    let (id, cookie, _) = app.member("m@example.com", &["roles:manage"]).await;
    for (method, path) in [
        (Method::GET, "/api/v1/users".to_owned()),
        (Method::PATCH, format!("/api/v1/users/{id}")),
        (Method::DELETE, format!("/api/v1/users/{id}")),
        (Method::PUT, format!("/api/v1/users/{id}/permissions")),
    ] {
        let (status, _, body) = app.send(method, &path, Some(&cookie), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(body["requiredPermission"], "users:manage");
    }
}

#[tokio::test]
async fn ownership_moves_to_the_target_in_one_step() {
    let app = App::new().await;
    let old = users::enabled_owners(&app.state.db)
        .await
        .unwrap()
        .remove(0);
    let (target, target_cookie, _) = app.member("new@example.com", &[]).await;
    let (_, _, keep) = app.member("keep@example.com", &[]).await;
    let body = json!({ "targetUserId": target, "password": PASSWORD, "keepRoleId": keep });
    let (status, reply) = app
        .owner(Method::POST, "/api/v1/owner/transfer", Some(body))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{reply}");

    let (status, _, _) = app
        .send(Method::GET, "/api/v1/users", Some(&target_cookie), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let owners = users::enabled_owners(&app.state.db).await.unwrap();
    assert_eq!(owners, std::slice::from_ref(&target));
    let old = users::find(&app.state.db, &old).await.unwrap().unwrap();
    assert_eq!(old.roles.iter().map(|r| r.id).collect::<Vec<_>>(), [keep]);
    // The old Owner's session now has the kept role only.
    let (status, _) = app.owner(Method::GET, "/api/v1/users", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, log) = app
        .send(
            Method::GET,
            "/api/v1/audit-log?action=owner.transferred",
            Some(&target_cookie),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(log["items"][0]["action"], "owner.transferred", "{log}");
}

#[tokio::test]
async fn a_transfer_is_refused_without_the_owner_role_or_the_password() {
    let app = App::new().await;
    let (target, _, _) = app.member("new@example.com", &[]).await;
    let (_, cookie, _) = app.member("other@example.com", &["users:manage"]).await;
    let body = json!({ "targetUserId": target, "password": PASSWORD });

    let (status, _, reply) = app
        .send(
            Method::POST,
            "/api/v1/owner/transfer",
            Some(&cookie),
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["code"], "owner_only");

    let wrong = json!({ "targetUserId": target, "password": "not the password" });
    let (status, reply) = app
        .owner(Method::POST, "/api/v1/owner/transfer", Some(wrong))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["code"], "wrong_password");

    let owner = users::enabled_owners(&app.state.db)
        .await
        .unwrap()
        .remove(0);
    let me = json!({ "targetUserId": owner, "password": PASSWORD });
    let (status, _) = app
        .owner(Method::POST, "/api/v1/owner/transfer", Some(me))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Nothing changed.
    assert_eq!(users::enabled_owners(&app.state.db).await.unwrap(), [owner]);
}

#[tokio::test]
async fn a_disabled_target_is_refused() {
    let app = App::new().await;
    let (target, _, _) = app.member("new@example.com", &[]).await;
    let (status, _) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{target}"),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let body = json!({ "targetUserId": target, "password": PASSWORD });
    let (status, reply) = app
        .owner(Method::POST, "/api/v1/owner/transfer", Some(body))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "target_disabled");
    assert_eq!(users::enabled_owners(&app.state.db).await.unwrap().len(), 1);
}
