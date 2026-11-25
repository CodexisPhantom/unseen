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

const PATH: &str = "/api/v1/settings/naming";
const PREVIEW: &str = "/api/v1/settings/naming/preview";

#[tokio::test]
async fn defaults_are_radarrs_and_the_preview_uses_radarrs_example_movie() {
    let app = App::new().await;
    let (status, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        reply,
        json!({
            "movieFolder": "{Movie Title} ({Release Year})",
            "movieFile": "{Movie Title} ({Release Year}) {Quality Full}",
            "renameMovies": false,
            "replaceIllegalCharacters": true,
            "colonReplacement": "smart",
            "overridden": []
        })
    );

    let (status, reply) = app.owner(Method::POST, PREVIEW, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["movieFolderExample"], "The Movie - Title (2010)");
    assert_eq!(
        reply["movieFileExample"],
        "The Movie - Title (2010) Bluray-1080p Proper"
    );
}

#[tokio::test]
async fn a_change_is_stored_and_a_partial_change_keeps_the_rest() {
    let app = App::new().await;
    let (status, reply) = app
        .owner(
            Method::PUT,
            PATH,
            Some(json!({
                "movieFolder": "{Movie.TitleThe}.{Release.Year}",
                "movieFile": "{Movie CleanTitle} {[Quality Title]}{-Release Group}",
                "renameMovies": true,
                "colonReplacement": "delete"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["movieFolder"], "{Movie.TitleThe}.{Release.Year}");
    assert_eq!(reply["renameMovies"], true);
    assert_eq!(reply["colonReplacement"], "delete");

    let (_, reply) = app
        .owner(
            Method::PUT,
            PATH,
            Some(json!({ "replaceIllegalCharacters": false })),
        )
        .await;
    assert_eq!(reply["replaceIllegalCharacters"], false);
    let (_, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(reply["movieFolder"], "{Movie.TitleThe}.{Release.Year}");
    assert_eq!(reply["renameMovies"], true);

    let (_, reply) = app.owner(Method::POST, PREVIEW, Some(json!({}))).await;
    assert_eq!(reply["movieFolderExample"], "Movie.Title,.The.2010");
    assert_eq!(
        reply["movieFileExample"],
        "The Movie Title [Bluray-1080p]-EVOLVE"
    );
}

#[tokio::test]
async fn saving_an_unknown_token_is_a_422_and_stores_nothing() {
    let app = App::new().await;
    for (field, template, offender) in [
        (
            "movieFolder",
            "{Movie Titel} ({Release Year})",
            "{Movie Titel}",
        ),
        ("movieFile", "{Movie Title} {Season}", "{Season}"),
        ("movieFile", "{Series Title}", "{Series Title}"),
    ] {
        let (status, reply) = app
            .owner(Method::PUT, PATH, Some(json!({ field: template })))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "validation");
        assert_eq!(reply["errors"][0]["field"], field);
        assert!(
            reply["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains(offender),
            "{reply}"
        );
    }
    for body in [
        json!({ "movieFile": "{Movie Title:30}" }),
        json!({ "movieFile": "   " }),
        json!({ "movieFolder": "/" }),
        json!({ "colonReplacement": "sideways" }),
    ] {
        let (status, reply) = app.owner(Method::PUT, PATH, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    }
    let (_, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(
        reply["movieFile"],
        "{Movie Title} ({Release Year}) {Quality Full}"
    );
    assert_eq!(reply["movieFolder"], "{Movie Title} ({Release Year})");
}

#[tokio::test]
async fn the_preview_shows_unsaved_templates_and_refuses_unknown_tokens() {
    let app = App::new().await;
    let (status, reply) = app
        .owner(
            Method::POST,
            PREVIEW,
            Some(json!({
                "movieFile": "{Movie Title} {Edition Tags} {MediaInfo Simple}",
                "colonReplacement": "dash"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        reply["movieFileExample"],
        "The Movie- Title Ultimate Extended Edition x264 DTS"
    );
    // Nothing was saved.
    let (_, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(reply["colonReplacement"], "smart");

    let (status, _) = app
        .owner(
            Method::POST,
            PREVIEW,
            Some(json!({ "movieFile": "{Nope}" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn read_and_preview_need_settings_read_and_save_needs_settings_manage() {
    let app = App::new().await;
    let (_, reader, _) = app.member("reader@example.com", &["settings:read"]).await;
    let (_, nobody, _) = app.member("nobody@example.com", &["library:read"]).await;

    let (status, _, _) = app.send(Method::GET, PATH, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app
        .send(Method::POST, PREVIEW, Some(&reader), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, reply) = app
        .send(
            Method::PUT,
            PATH,
            Some(&reader),
            Some(json!({ "renameMovies": true })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["requiredPermission"], "settings:manage");

    for (method, path, body) in [
        (Method::GET, PATH, None),
        (Method::POST, PREVIEW, Some(json!({}))),
    ] {
        let (status, _, _) = app.send(method, path, Some(&nobody), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
