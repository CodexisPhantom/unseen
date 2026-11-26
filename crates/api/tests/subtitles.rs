use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
    /// `tmdb` is the mock server that stands in for TMDB.
    async fn new(tmdb: &MockServer) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let mut state = AppState::new(db, "test secret", None).await.unwrap();
        state.tmdb_base_url = tmdb.uri();
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

const MOVIE: &str = include_str!("../../../fixtures/tmdb/movie_603.json");

const KEY: &str = "fixture-key-0123456789abcdef";

struct Library {
    app: App,
    root: i64,
    hd: i64,
    _dir: TempDir,
    movie_dir: std::path::PathBuf,
}

/// An app with a TMDB key, one root folder, and the folder `The Matrix
/// (1999)` with the given files (name and content).
async fn library(server: &MockServer, files: &[&str]) -> Library {
    Mock::given(method("GET"))
        .and(path("/3/movie/603"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(MOVIE.as_bytes().to_vec(), "application/json"),
        )
        .mount(server)
        .await;
    let app = App::new(server).await;
    db::settings::set_secret(
        &app.state.db,
        &app.state.secrets,
        "tmdb.api_key",
        &db::secret::Secret::new(KEY),
    )
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let movie_dir = dir.path().join("The Matrix (1999)");
    for file in files {
        let path = movie_dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }
    let (status, root) = app
        .owner(
            Method::POST,
            "/api/v1/root-folders",
            Some(json!({ "path": dir.path().to_str().unwrap() })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{root}");
    let (_, profiles) = app
        .owner(Method::GET, "/api/v1/quality-profiles", None)
        .await;
    let hd = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "HD-1080p")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    Library {
        root: root["id"].as_i64().unwrap(),
        hd,
        app,
        _dir: dir,
        movie_dir,
    }
}

async fn adopt(lib: &Library) -> i64 {
    let (status, reply) = lib
        .app
        .owner(
            Method::POST,
            "/api/v1/library/adopt",
            Some(json!({
                "rootFolderId": lib.root,
                "qualityProfileIds": [lib.hd],
                "movies": [{ "folderName": "The Matrix (1999)", "tmdbId": 603 }],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    reply["adopted"][0]["movieId"].as_i64().unwrap()
}

/// `language/kind/path` of each listed subtitle.
fn listed(reply: &Value) -> Vec<String> {
    reply["subtitles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{}/{}/{}",
                s["language"].as_str().unwrap(),
                s["kind"].as_str().unwrap(),
                s["relativePath"].as_str().unwrap()
            )
        })
        .collect()
}

const VIDEO: &str = "The Matrix (1999) Bluray-1080p.mkv";

#[tokio::test]
async fn existing_srt_files_next_to_a_movie_are_listed_with_their_language() {
    let server = MockServer::start().await;
    let lib = library(
        &server,
        &[
            VIDEO,
            "The Matrix (1999) Bluray-1080p.en.srt",
            "The Matrix (1999) Bluray-1080p.fr.forced.srt",
            "The Matrix (1999) Bluray-1080p.eng.sdh.srt",
            "The Matrix (1999) Bluray-1080p.pt-BR.srt",
            "The Matrix (1999) Bluray-1080p.srt",
            // Not subtitles of this video.
            "Other.en.srt",
            "The Matrix (1999) Bluray-1080p.nfo",
            "Subs/The Matrix (1999) Bluray-1080p.de.srt",
        ],
    )
    .await;
    let id = adopt(&lib).await;

    let (status, reply) = lib
        .app
        .owner(Method::GET, &format!("/api/v1/movies/{id}/subtitles"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["languageProfileId"], Value::Null);
    let base = "The Matrix (1999) Bluray-1080p";
    assert_eq!(
        listed(&reply),
        [
            format!("en/hi/{base}.eng.sdh.srt"),
            format!("en/normal/{base}.en.srt"),
            format!("fr/forced/{base}.fr.forced.srt"),
            format!("pt-BR/normal/{base}.pt-BR.srt"),
            format!("und/normal/{base}.srt"),
        ]
    );
    // Found next to the video, so no provider.
    assert_eq!(reply["subtitles"][0]["provider"], Value::Null);
}

#[tokio::test]
async fn a_scan_forgets_deleted_files_and_adds_new_ones_without_touching_the_disk() {
    let server = MockServer::start().await;
    let lib = library(
        &server,
        &[
            VIDEO,
            "The Matrix (1999) Bluray-1080p.en.srt",
            "The Matrix (1999) Bluray-1080p.fr.srt",
        ],
    )
    .await;
    let id = adopt(&lib).await;
    fs::remove_file(lib.movie_dir.join("The Matrix (1999) Bluray-1080p.fr.srt")).unwrap();
    fs::write(
        lib.movie_dir.join("The Matrix (1999) Bluray-1080p.es.ass"),
        "x",
    )
    .unwrap();

    let (status, reply) = lib
        .app
        .owner(
            Method::POST,
            &format!("/api/v1/movies/{id}/subtitles/scan"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        listed(&reply),
        [
            "en/normal/The Matrix (1999) Bluray-1080p.en.srt",
            "es/normal/The Matrix (1999) Bluray-1080p.es.ass",
        ]
    );
    let mut on_disk: Vec<_> = fs::read_dir(&lib.movie_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    assert_eq!(on_disk.len(), 3);

    let (status, _) = lib
        .app
        .owner(Method::POST, "/api/v1/movies/999/subtitles/scan", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn profile(name: &str) -> Value {
    json!({
        "name": name,
        "items": [
            { "language": "en", "kind": "normal" },
            { "language": "fr", "kind": "forced" },
        ],
        "cutoff": 0,
    })
}

#[tokio::test]
async fn language_profiles_are_created_changed_and_deleted() {
    let server = MockServer::start().await;
    let lib = library(&server, &[VIDEO]).await;
    let app = &lib.app;
    let (status, created) = app
        .owner(
            Method::POST,
            "/api/v1/language-profiles",
            Some(profile("English")),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["items"][1]["kind"], "forced");
    assert_eq!(created["cutoff"], 0);
    let pid = created["id"].as_i64().unwrap();

    let (status, again) = app
        .owner(
            Method::POST,
            "/api/v1/language-profiles",
            Some(profile("English")),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "name_taken");

    for (field, mut bad) in [
        ("name", profile(" ")),
        ("items", json!({ "name": "x", "items": [] })),
        (
            "items",
            json!({ "name": "x", "items": [{ "language": "english", "kind": "normal" }] }),
        ),
        (
            "items",
            json!({ "name": "x", "items": [
                { "language": "en", "kind": "hi" }, { "language": "en", "kind": "hi" }] }),
        ),
        (
            "cutoff",
            json!({ "name": "x", "items": [{ "language": "en", "kind": "normal" }], "cutoff": 1 }),
        ),
        (
            "cutoff",
            json!({ "name": "x", "items": [{ "language": "en", "kind": "normal" }], "cutoff": -1 }),
        ),
    ] {
        let (status, reply) = app
            .owner(Method::POST, "/api/v1/language-profiles", Some(bad.take()))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {reply}");
        assert_eq!(reply["errors"][0]["field"], field, "{reply}");
    }
    // An unknown kind never reaches the rules.
    let (status, _) = app
        .owner(
            Method::POST,
            "/api/v1/language-profiles",
            Some(json!({ "name": "x", "items": [{ "language": "en", "kind": "sdh" }] })),
        )
        .await;
    assert!(status.is_client_error());

    // Replace: the cutoff goes away.
    let mut changed = profile("English and French");
    changed["cutoff"] = Value::Null;
    let (status, updated) = app
        .owner(
            Method::PUT,
            &format!("/api/v1/language-profiles/{pid}"),
            Some(changed),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "English and French");
    assert_eq!(updated["cutoff"], Value::Null);
    let (status, _) = app
        .owner(
            Method::PUT,
            "/api/v1/language-profiles/999",
            Some(profile("Z")),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, list) = app
        .owner(Method::GET, "/api/v1/language-profiles", None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // A profile that a movie uses cannot be deleted.
    let id = adopt(&lib).await;
    let (status, reply) = app
        .owner(
            Method::PUT,
            &format!("/api/v1/movies/{id}/language-profile"),
            Some(json!({ "languageProfileId": pid })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["languageProfileId"], pid);
    let (status, reply) = app
        .owner(
            Method::DELETE,
            &format!("/api/v1/language-profiles/{pid}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "profile_in_use");

    let (status, reply) = app
        .owner(
            Method::PUT,
            &format!("/api/v1/movies/{id}/language-profile"),
            Some(json!({ "languageProfileId": 999 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    let (status, _) = app
        .owner(
            Method::PUT,
            "/api/v1/movies/999/language-profile",
            Some(json!({ "languageProfileId": null })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, reply) = app
        .owner(
            Method::PUT,
            &format!("/api/v1/movies/{id}/language-profile"),
            Some(json!({ "languageProfileId": null })),
        )
        .await;
    assert_eq!(reply["languageProfileId"], Value::Null);
    let (status, _) = app
        .owner(
            Method::DELETE,
            &format!("/api/v1/language-profiles/{pid}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app
        .owner(
            Method::DELETE,
            &format!("/api/v1/language-profiles/{pid}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_default_profile_setting_must_name_a_profile() {
    let server = MockServer::start().await;
    let lib = library(&server, &[VIDEO]).await;
    let app = &lib.app;
    let (status, reply) = app
        .owner(Method::GET, "/api/v1/settings/subtitles", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["defaultProfileId"], Value::Null);

    let (status, reply) = app
        .owner(
            Method::PUT,
            "/api/v1/settings/subtitles",
            Some(json!({ "defaultProfileId": 7 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");

    let (_, created) = app
        .owner(
            Method::POST,
            "/api/v1/language-profiles",
            Some(profile("English")),
        )
        .await;
    let pid = created["id"].as_i64().unwrap();
    let (status, reply) = app
        .owner(
            Method::PUT,
            "/api/v1/settings/subtitles",
            Some(json!({ "defaultProfileId": pid })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let (_, reply) = app
        .owner(Method::GET, "/api/v1/settings/subtitles", None)
        .await;
    assert_eq!(reply["defaultProfileId"], pid);

    // Deleting the default profile clears the default.
    app.owner(
        Method::DELETE,
        &format!("/api/v1/language-profiles/{pid}"),
        None,
    )
    .await;
    let (_, reply) = app
        .owner(Method::GET, "/api/v1/settings/subtitles", None)
        .await;
    assert_eq!(reply["defaultProfileId"], Value::Null);
}

#[tokio::test]
async fn reading_needs_library_read_and_writing_needs_subtitles_manage() {
    let server = MockServer::start().await;
    let lib = library(&server, &[VIDEO]).await;
    let id = adopt(&lib).await;
    let (_, reader, _) = lib
        .app
        .member("reader@example.com", &["library:read"])
        .await;
    let (_, nobody, _) = lib.app.member("nobody@example.com", &["queue:read"]).await;
    let (_, manager, _) = lib
        .app
        .member("manager@example.com", &["subtitles:manage"])
        .await;

    for path in [
        "/api/v1/language-profiles".to_owned(),
        "/api/v1/settings/subtitles".to_owned(),
        format!("/api/v1/movies/{id}/subtitles"),
    ] {
        let (status, _, _) = lib.app.send(Method::GET, &path, Some(&reader), None).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let (status, _, reply) = lib.app.send(Method::GET, &path, Some(&nobody), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(reply["requiredPermission"], "library:read");
    }
    let (status, _, reply) = lib
        .app
        .send(
            Method::POST,
            "/api/v1/language-profiles",
            Some(&reader),
            Some(profile("English")),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["requiredPermission"], "subtitles:manage");
    let (status, _, _) = lib
        .app
        .send(
            Method::POST,
            "/api/v1/language-profiles",
            Some(&manager),
            Some(profile("English")),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}
