use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};

use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

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

const PATH: &str = "/api/v1/quality-definitions";
const BLURAY_1080P: i32 = 7;

fn one(path: &str) -> String {
    format!("{PATH}/{path}")
}

#[tokio::test]
async fn lists_every_quality_in_the_default_order_with_radarrs_defaults() {
    let app = App::new().await;
    let (status, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(status, StatusCode::OK);
    let list = reply.as_array().unwrap();
    assert_eq!(list.len(), 30);
    let names: Vec<_> = list.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(names[0], "Unknown");
    assert_eq!(names[5], "REGIONAL");
    assert_eq!(names[6], "DVDSCR");
    assert_eq!(names[29], "Raw-HD");

    let by_name = |name: &str| list.iter().find(|d| d["name"] == name).unwrap();
    assert_eq!(
        by_name("Bluray-720p"),
        &json!({
            "qualityId": 6,
            "name": "Bluray-720p",
            "title": "Bluray-720p",
            "minSize": 0.0,
            "preferredSize": 95.0,
            "maxSize": 100.0
        })
    );
    // Radarr gives no limit from Bluray-1080p up.
    let remux = by_name("Remux-2160p");
    assert_eq!(remux["preferredSize"], Value::Null);
    assert_eq!(remux["maxSize"], Value::Null);
}

#[tokio::test]
async fn a_change_keeps_what_is_absent_and_null_clears_the_optional_sizes() {
    let app = App::new().await;
    let path = one(&BLURAY_1080P.to_string());

    let (status, reply) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "minSize": 20.0, "preferredSize": 60.0, "maxSize": 120.0 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["minSize"], 20.0);
    assert_eq!(reply["maxSize"], 120.0);
    assert_eq!(reply["title"], "Bluray-1080p");

    // Only the title: the sizes stay.
    let (_, reply) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "title": " Blu-ray 1080p " })),
        )
        .await;
    assert_eq!(reply["title"], "Blu-ray 1080p");
    assert_eq!(reply["preferredSize"], 60.0);

    // `null` removes the maximum: no limit.
    let (status, reply) = app
        .owner(Method::PATCH, &path, Some(json!({ "maxSize": null })))
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["maxSize"], Value::Null);
    assert_eq!(reply["preferredSize"], 60.0);

    // The list shows the change, and the audit log records it.
    let (_, list) = app.owner(Method::GET, PATH, None).await;
    let stored = list
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["qualityId"] == BLURAY_1080P)
        .unwrap();
    assert_eq!(stored["title"], "Blu-ray 1080p");
    let (_, log) = app
        .owner(
            Method::GET,
            "/api/v1/audit-log?action=quality_definition.updated",
            None,
        )
        .await;
    assert_eq!(log["total"], 3);
    assert_eq!(log["items"][0]["targetKind"], "quality_definition");
    assert_eq!(log["items"][0]["data"]["maxSize"]["new"], Value::Null);
}

#[tokio::test]
async fn bad_limits_and_unknown_qualities_are_refused() {
    let app = App::new().await;
    let path = one(&BLURAY_1080P.to_string());
    for (body, field) in [
        (json!({ "minSize": -1.0 }), "minSize"),
        (json!({ "maxSize": 0.0 }), "maxSize"),
        (json!({ "minSize": 50.0, "maxSize": 40.0 }), "maxSize"),
        (
            json!({ "minSize": 10.0, "preferredSize": 5.0 }),
            "preferredSize",
        ),
        (
            json!({ "maxSize": 10.0, "preferredSize": 20.0 }),
            "preferredSize",
        ),
        (json!({ "title": "  " }), "title"),
    ] {
        let (status, reply) = app.owner(Method::PATCH, &path, Some(body.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {reply}");
        assert!(reply.to_string().contains(field), "{body}: {reply}");
    }
    // Nothing was stored.
    let (_, list) = app.owner(Method::GET, PATH, None).await;
    let stored = list
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["qualityId"] == BLURAY_1080P)
        .unwrap();
    assert_eq!(stored["minSize"], 0.0);

    for id in ["999", "11"] {
        let (status, _) = app
            .owner(Method::PATCH, &one(id), Some(json!({ "minSize": 1.0 })))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}");
    }
}

#[tokio::test]
async fn reading_needs_profiles_read_and_writing_needs_profiles_manage() {
    let app = App::new().await;
    let (_, reader, _) = app.member("reader@example.com", &["profiles:read"]).await;
    let (status, _, _) = app.send(Method::GET, PATH, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, reply) = app
        .send(
            Method::PATCH,
            &one("7"),
            Some(&reader),
            Some(json!({ "minSize": 1.0 })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["requiredPermission"], "profiles:manage");

    let (_, nobody, _) = app.member("nobody@example.com", &[]).await;
    let (status, _, reply) = app.send(Method::GET, PATH, Some(&nobody), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["requiredPermission"], "profiles:read");
}
