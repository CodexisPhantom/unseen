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

const PATH: &str = "/api/v1/indexers";
const KEY: &str = "sup3r-s3cret-key";

fn body(name: &str) -> Value {
    json!({ "name": name, "url": "http://indexer.test/api", "apiKey": KEY })
}

async fn create(app: &App, name: &str) -> i64 {
    let (status, reply) = app.owner(Method::POST, PATH, Some(body(name))).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply["id"].as_i64().unwrap()
}

#[tokio::test]
async fn create_get_list_update_and_delete() {
    let app = App::new().await;
    let id = create(&app, "Alpha").await;
    let path = format!("{PATH}/{id}");

    let (status, reply) = app.owner(Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["kind"], "torznab");
    assert_eq!(reply["priority"], 25);
    assert_eq!(reply["useProxy"], true);
    assert_eq!(reply["enabled"], true);
    assert_eq!(reply["hasApiKey"], true);
    assert_eq!(reply["apiKeyMustBeReentered"], false);
    assert_eq!(reply["tags"], json!([]));

    let (status, reply) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "priority": 10, "categories": [2000, 2040], "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["priority"], 10);
    assert_eq!(reply["categories"], json!([2000, 2040]));
    assert_eq!(reply["enabled"], false);

    create(&app, "Beta").await;
    let (_, reply) = app.owner(Method::GET, PATH, None).await;
    let names: Vec<_> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Alpha", "Beta"]);

    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.owner(Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_api_key_is_never_returned_or_logged_and_a_rename_keeps_it() {
    let app = App::new().await;
    let id = create(&app, "Alpha").await;
    let path = format!("{PATH}/{id}");

    let stored = db::indexers::find(&app.state.db, id as i32)
        .await
        .unwrap()
        .unwrap()
        .model
        .api_key;
    assert!(!stored.contains(KEY), "stored encrypted");
    let key_before = db::secret::decrypt(&app.state.secrets, &stored).unwrap();
    assert_eq!(key_before.expose(), KEY);

    let (status, patched) = app
        .owner(Method::PATCH, &path, Some(json!({ "name": "Renamed" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, one) = app.owner(Method::GET, &path, None).await;
    let (_, all) = app.owner(Method::GET, PATH, None).await;
    let (_, created) = app.owner(Method::POST, PATH, Some(body("Other"))).await;
    let (status, changed) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "apiKey": "brand-new-key" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, audit) = app.owner(Method::GET, "/api/v1/audit-log", None).await;
    assert_eq!(status, StatusCode::OK);
    for reply in [&patched, &one, &all, &created, &changed, &audit] {
        let text = reply.to_string();
        assert!(
            !text.contains(KEY) && !text.contains("brand-new-key"),
            "{text}"
        );
    }
    assert!(audit.to_string().contains("indexer.updated"));

    // The rename kept the key; the later update replaced it.
    let after_rename = db::indexers::find(&app.state.db, id as i32)
        .await
        .unwrap()
        .unwrap()
        .model
        .api_key;
    assert_eq!(
        db::secret::decrypt(&app.state.secrets, &after_rename)
            .unwrap()
            .expose(),
        "brand-new-key"
    );
}

#[tokio::test]
async fn a_name_only_update_keeps_the_stored_key() {
    let app = App::new().await;
    let id = create(&app, "Alpha").await;
    let key = |app: &App| {
        let db = app.state.db.clone();
        async move {
            db::indexers::find(&db, id as i32)
                .await
                .unwrap()
                .unwrap()
                .model
                .api_key
        }
    };
    let before = key(&app).await;
    let (status, _) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{id}"),
            Some(json!({ "name": "Renamed" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let after = key(&app).await;
    assert_eq!(before, after);
    assert_eq!(
        db::secret::decrypt(&app.state.secrets, &after)
            .unwrap()
            .expose(),
        KEY
    );
}

#[tokio::test]
async fn rejects_bad_input_and_duplicate_names() {
    let app = App::new().await;
    create(&app, "Alpha").await;
    let (status, reply) = app.owner(Method::POST, PATH, Some(body("Alpha"))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "name_taken");

    for (field, bad) in [
        ("name", json!({ "name": " " })),
        ("url", json!({ "url": "ftp://x.test/" })),
        ("url", json!({ "url": "http://169.254.169.254/" })),
        ("url", json!({ "url": "not a url" })),
        ("apiKey", json!({ "apiKey": " " })),
        ("priority", json!({ "priority": 0 })),
        ("priority", json!({ "priority": 51 })),
        ("kind", json!({ "kind": "newznab" })),
        ("tags", json!({ "tags": [999] })),
    ] {
        let mut request = body("New");
        request
            .as_object_mut()
            .unwrap()
            .extend(bad.as_object().unwrap().clone());
        let (status, reply) = app.owner(Method::POST, PATH, Some(request)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {reply}");
        assert_eq!(reply["errors"][0]["field"], field);
    }
}

#[tokio::test]
async fn tags_are_set_and_replaced() {
    let app = App::new().await;
    let tag = db::entities::tags::ActiveModel {
        label: Set("4k".into()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    let mut request = body("Alpha");
    request["tags"] = json!([tag.id]);
    let (status, reply) = app.owner(Method::POST, PATH, Some(request)).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    assert_eq!(reply["tags"], json!([tag.id]));

    let (_, reply) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{}", reply["id"]),
            Some(json!({ "tags": [] })),
        )
        .await;
    assert_eq!(reply["tags"], json!([]));
}

#[tokio::test]
async fn reading_needs_indexers_read_and_writing_needs_indexers_manage() {
    let app = App::new().await;
    let id = create(&app, "Alpha").await;
    let (_, reader, _) = app.member("reader@example.com", &["indexers:read"]).await;
    let path = format!("{PATH}/{id}");

    for (method, path, payload) in [
        (Method::GET, PATH.to_owned(), None),
        (Method::GET, path.clone(), None),
    ] {
        let (status, _, _) = app.send(method, &path, Some(&reader), payload).await;
        assert_eq!(status, StatusCode::OK);
    }
    for (method, path, payload) in [
        (Method::POST, PATH.to_owned(), Some(body("New"))),
        (Method::PATCH, path.clone(), Some(json!({ "name": "x" }))),
        (Method::DELETE, path.clone(), None),
    ] {
        let (status, _, reply) = app.send(method, &path, Some(&reader), payload).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(reply["requiredPermission"], "indexers:manage");
    }
}
