use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{movie_profiles, movies, root_folders};
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

const PATH: &str = "/api/v1/quality-profiles";

fn body(name: &str) -> Value {
    json!({
        "name": name,
        "cutoff": 7,
        "items": [
            { "type": "quality", "qualityId": 4, "allowed": true },
            { "type": "quality", "qualityId": 7, "allowed": true },
            { "type": "quality", "qualityId": 30, "allowed": false },
        ],
    })
}

async fn create(app: &App, name: &str) -> i64 {
    let (status, reply) = app.owner(Method::POST, PATH, Some(body(name))).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply["id"].as_i64().unwrap()
}

#[tokio::test]
async fn a_new_install_lists_the_seeded_profiles() {
    let app = App::new().await;
    let (status, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<_> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Any",
            "HD - 720p/1080p",
            "HD-1080p",
            "HD-720p",
            "SD",
            "Ultra-HD"
        ]
    );
    let hd = &reply[2];
    assert_eq!(hd["language"], "any");
    assert_eq!(hd["upgradeAllowed"], true);
    assert_eq!(hd["cutoff"], 7);
}

#[tokio::test]
async fn create_get_update_and_delete() {
    let app = App::new().await;
    let id = create(&app, "Mine").await;
    let path = format!("{PATH}/{id}");

    let (status, reply) = app.owner(Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["language"], "any");
    assert_eq!(reply["minUpgradeFormatScore"], 1);
    assert_eq!(
        reply["items"][1],
        json!({ "type": "quality", "qualityId": 7, "allowed": true })
    );

    let (status, reply) = app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "language": "original", "upgradeAllowed": false, "cutoff": 4 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["language"], "original");
    assert_eq!(reply["cutoff"], 4);
    assert_eq!(reply["items"].as_array().unwrap().len(), 3);

    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.owner(Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_cutoff_that_is_not_an_allowed_quality_is_rejected() {
    let app = App::new().await;
    for cutoff in [30, 12, 9999] {
        let mut profile = body("Bad");
        profile["cutoff"] = json!(cutoff);
        let (status, reply) = app.owner(Method::POST, PATH, Some(profile)).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{cutoff}: {reply}"
        );
        assert_eq!(reply["errors"][0]["field"], "cutoff");
    }
    // The same rule holds when a patch makes the cutoff invalid.
    let id = create(&app, "Mine").await;
    let (status, _) = app
        .owner(
            Method::PATCH,
            &format!("{PATH}/{id}"),
            Some(json!({ "cutoff": 30 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn names_languages_and_duplicate_names_are_checked() {
    let app = App::new().await;
    let (status, _) = app.owner(Method::POST, PATH, Some(body("  "))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut profile = body("Lang");
    profile["language"] = json!("Klingon");
    let (status, _) = app.owner(Method::POST, PATH, Some(profile)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, reply) = app.owner(Method::POST, PATH, Some(body("SD"))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "name_taken");
}

#[tokio::test]
async fn a_profile_used_by_a_movie_cannot_be_deleted() {
    let app = App::new().await;
    let id = create(&app, "Used").await;
    let root = root_folders::ActiveModel {
        path: Set("/movies".into()),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    let movie = movies::ActiveModel {
        tmdb_id: Set(1),
        title: Set("Movie".into()),
        sort_title: Set("movie".into()),
        year: Set(2020),
        status: Set("released".into()),
        root_folder_id: Set(root.id),
        folder_name: Set("Movie (2020)".into()),
        monitored: Set(true),
        minimum_availability: Set("released".into()),
        added_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    movie_profiles::ActiveModel {
        movie_id: Set(movie.id),
        quality_profile_id: Set(id as i32),
        is_primary: Set(true),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let (status, reply) = app
        .owner(Method::DELETE, &format!("{PATH}/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "profile_in_use");
}

#[tokio::test]
async fn read_and_manage_are_separate_permissions() {
    let app = App::new().await;
    let (_, reader, _) = app.member("read@example.com", &["profiles:read"]).await;
    let (_, nobody, _) = app.member("none@example.com", &[]).await;

    let (status, _, _) = app.send(Method::GET, PATH, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app
        .send(Method::POST, PATH, Some(&reader), Some(body("X")))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = app
        .send(Method::DELETE, &format!("{PATH}/1"), Some(&reader), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = app.send(Method::GET, PATH, Some(&nobody), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
