use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{movie_files, role_permissions, roles, user_roles};
use serde_json::{Value, json};
use std::path::PathBuf;
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
const NOT_FOUND: &str = include_str!("../../../fixtures/tmdb/error_not_found.json");

const MOVIES: &str = "/api/v1/movies";
const KEY: &str = "fixture-key-0123456789abcdef";

fn json_body(body: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

/// An app whose TMDB key is set, with a real root folder and two profiles.
struct Library {
    app: App,
    root: i64,
    hd: i64,
    uhd: i64,
    dir: TempDir,
}

async fn library(server: &MockServer) -> Library {
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
    let id = |name: &str| {
        profiles
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    Library {
        root: root["id"].as_i64().unwrap(),
        hd: id("HD-1080p"),
        uhd: id("Ultra-HD"),
        app,
        dir,
    }
}

async fn tmdb_movie(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/3/movie/603"))
        .respond_with(json_body(MOVIE, 200))
        .mount(server)
        .await;
}

fn add_body(lib: &Library) -> Value {
    json!({ "tmdbId": 603, "rootFolderId": lib.root, "qualityProfileIds": [lib.hd] })
}

async fn add(lib: &Library) -> Value {
    let (status, reply) = lib
        .app
        .owner(Method::POST, MOVIES, Some(add_body(lib)))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply
}

#[tokio::test]
async fn adding_a_movie_fetches_metadata_and_names_the_folder_like_radarr() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;

    let movie = add(&lib).await;
    assert_eq!(movie["title"], "The Matrix");
    assert_eq!(movie["sortTitle"], "matrix");
    assert_eq!(movie["year"], 1999);
    assert_eq!(movie["imdbId"], "tt0133093");
    assert_eq!(movie["tmdbId"], 603);
    assert_eq!(movie["folderName"], "The Matrix (1999)");
    assert_eq!(movie["status"], "released");
    assert_eq!(movie["monitored"], true);
    assert_eq!(movie["minimumAvailability"], "released");
    assert_eq!(movie["runtime"], 136);
    assert_eq!(movie["profiles"][0]["qualityProfileId"], lib.hd);
    assert_eq!(movie["profiles"][0]["isPrimary"], true);
    assert_eq!(movie["profiles"][0]["fileId"], Value::Null);
    // Adding creates nothing on disk.
    assert_eq!(std::fs::read_dir(lib.dir.path()).unwrap().count(), 0);

    let (status, listed) = lib.app.owner(Method::GET, MOVIES, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let (status, one) = lib
        .app
        .owner(Method::GET, &format!("{MOVIES}/{}", movie["id"]), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["title"], "The Matrix");
    let (status, _) = lib
        .app
        .owner(Method::GET, &format!("{MOVIES}/999"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn adding_a_movie_twice_is_a_409_before_tmdb_is_called_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/3/movie/603"))
        .respond_with(json_body(MOVIE, 200))
        .expect(1)
        .mount(&server)
        .await;
    let lib = library(&server).await;
    add(&lib).await;

    let (status, reply) = lib
        .app
        .owner(Method::POST, MOVIES, Some(add_body(&lib)))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "movie_exists");
}

#[tokio::test]
async fn adding_takes_several_profiles_tags_and_options() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let (_, tag) = lib
        .app
        .owner(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "label": "Kids" })),
        )
        .await;
    assert_eq!(tag["label"], "kids");

    let (status, movie) = lib
        .app
        .owner(
            Method::POST,
            MOVIES,
            Some(json!({
                "tmdbId": 603,
                "rootFolderId": lib.root,
                "qualityProfileIds": [lib.uhd, lib.hd],
                "monitored": false,
                "minimumAvailability": "inCinemas",
                "tags": [tag["id"]],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{movie}");
    assert_eq!(movie["monitored"], false);
    assert_eq!(movie["minimumAvailability"], "inCinemas");
    assert_eq!(movie["tags"], json!([tag["id"]]));
    assert_eq!(movie["profiles"][0]["qualityProfileId"], lib.uhd);
    assert_eq!(movie["profiles"][0]["isPrimary"], true);
    assert_eq!(movie["profiles"][1]["qualityProfileId"], lib.hd);
    assert_eq!(movie["profiles"][1]["isPrimary"], false);
}

#[tokio::test]
async fn add_rejects_bad_input_without_calling_tmdb() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json_body(MOVIE, 200))
        .expect(0)
        .mount(&server)
        .await;
    let lib = library(&server).await;

    for (field, change) in [
        ("qualityProfileIds", json!({ "qualityProfileIds": [] })),
        ("qualityProfileIds", json!({ "qualityProfileIds": [9999] })),
        ("rootFolderId", json!({ "rootFolderId": 9999 })),
        (
            "minimumAvailability",
            json!({ "minimumAvailability": "soon" }),
        ),
        ("tags", json!({ "tags": [9999] })),
    ] {
        let mut request = add_body(&lib);
        request
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let (status, reply) = lib.app.owner(Method::POST, MOVIES, Some(request)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {reply}");
        assert_eq!(reply["errors"][0]["field"], field);
    }
}

#[tokio::test]
async fn add_reports_tmdb_problems() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/3/movie/603"))
        .respond_with(json_body(NOT_FOUND, 404))
        .mount(&server)
        .await;
    let lib = library(&server).await;
    let (status, reply) = lib
        .app
        .owner(Method::POST, MOVIES, Some(add_body(&lib)))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply["errors"][0]["field"], "tmdbId");
}

#[tokio::test]
async fn patch_changes_monitored_availability_tags_and_the_primary_profile() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let path = format!("{MOVIES}/{}", movie["id"]);
    let (_, tag) = lib
        .app
        .owner(Method::POST, "/api/v1/tags", Some(json!({ "label": "4k" })))
        .await;

    let (status, patched) = lib
        .app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({
                "monitored": false,
                "minimumAvailability": "announced",
                "tags": [tag["id"]],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["monitored"], false);
    assert_eq!(patched["minimumAvailability"], "announced");
    assert_eq!(patched["tags"], json!([tag["id"]]));

    let (status, patched) = lib
        .app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "minimumAvailability": "nope" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{patched}");
    let (status, _) = lib
        .app
        .owner(Method::PATCH, &path, Some(json!({ "tags": [9999] })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = lib
        .app
        .owner(Method::PATCH, &format!("{MOVIES}/999"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_second_profile_can_be_added_and_the_primary_moves_before_it_is_removed() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let id = movie["id"].as_i64().unwrap();
    let path = format!("{MOVIES}/{id}");
    let first = movie["profiles"][0]["id"].as_i64().unwrap();

    // The only profile cannot go.
    let (status, reply) = lib
        .app
        .owner(Method::DELETE, &format!("{path}/profiles/{first}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "last_profile");

    let (status, reply) = lib
        .app
        .owner(
            Method::POST,
            &format!("{path}/profiles"),
            Some(json!({ "qualityProfileId": lib.uhd })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    assert_eq!(reply["profiles"].as_array().unwrap().len(), 2);
    let second = reply["profiles"][1]["id"].as_i64().unwrap();
    assert_eq!(reply["profiles"][1]["isPrimary"], false);

    let (status, again) = lib
        .app
        .owner(
            Method::POST,
            &format!("{path}/profiles"),
            Some(json!({ "qualityProfileId": lib.uhd })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    let (status, _) = lib
        .app
        .owner(
            Method::POST,
            &format!("{path}/profiles"),
            Some(json!({ "qualityProfileId": 9999 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Removing the primary one is refused until another is primary.
    let (status, reply) = lib
        .app
        .owner(Method::DELETE, &format!("{path}/profiles/{first}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "primary_profile");
    let (status, reply) = lib
        .app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "primaryProfileId": second })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["profiles"][0]["id"], second);
    let (status, _) = lib
        .app
        .owner(Method::DELETE, &format!("{path}/profiles/{first}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, reply) = lib.app.owner(Method::GET, &path, None).await;
    assert_eq!(reply["profiles"].as_array().unwrap().len(), 1);

    let (status, _) = lib
        .app
        .owner(Method::DELETE, &format!("{path}/profiles/9999"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = lib
        .app
        .owner(
            Method::PATCH,
            &path,
            Some(json!({ "primaryProfileId": 9999 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// The movie folder with one file, written to disk and recorded.
async fn with_file(lib: &Library, movie: &Value, name: &str, managed: bool) -> (i64, PathBuf) {
    let folder = lib.dir.path().join(movie["folderName"].as_str().unwrap());
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(name), b"video").unwrap();
    let profile = managed.then(|| movie["profiles"][0]["id"].as_i64().unwrap() as i32);
    let file = movie_files::ActiveModel {
        movie_id: Set(movie["id"].as_i64().unwrap() as i32),
        relative_path: Set(name.into()),
        size: Set(5),
        quality_id: Set(7),
        movie_profile_id: Set(profile),
        added_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&lib.app.state.db)
    .await
    .unwrap();
    (i64::from(file.id), folder)
}

#[tokio::test]
async fn files_are_listed_and_deleted_from_disk_and_library() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let path = format!("{MOVIES}/{}", movie["id"]);
    let (managed, folder) = with_file(&lib, &movie, "Matrix.mkv", true).await;
    let (unmanaged, _) = with_file(&lib, &movie, "Extended.mkv", false).await;

    let (status, files) = lib
        .app
        .owner(Method::GET, &format!("{path}/files"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let by_name = |name: &str| {
        files
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["relativePath"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("Matrix.mkv")["managed"], true);
    assert_eq!(by_name("Extended.mkv")["managed"], false);
    assert_eq!(by_name("Extended.mkv")["movieProfileId"], Value::Null);
    let (_, one) = lib.app.owner(Method::GET, &path, None).await;
    assert_eq!(one["profiles"][0]["fileId"], managed);

    let (status, _) = lib
        .app
        .owner(Method::DELETE, &format!("{path}/files/{unmanaged}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!folder.join("Extended.mkv").exists());
    assert!(folder.join("Matrix.mkv").exists(), "the other file stays");
    let (status, _) = lib
        .app
        .owner(Method::DELETE, &format!("{path}/files/{unmanaged}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, files) = lib
        .app
        .owner(Method::GET, &format!("{path}/files"), None)
        .await;
    assert_eq!(files.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn deleting_without_files_leaves_the_folder_untouched() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let (_, folder) = with_file(&lib, &movie, "Matrix.mkv", true).await;
    let path = format!("{MOVIES}/{}", movie["id"]);

    let (status, _) = lib.app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(folder.join("Matrix.mkv").exists());
    let (status, _) = lib.app.owner(Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = lib.app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_with_files_removes_the_folder_and_only_that_folder() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let (_, folder) = with_file(&lib, &movie, "Matrix.mkv", true).await;
    let neighbour = lib.dir.path().join("Other (2000)");
    std::fs::create_dir(&neighbour).unwrap();

    let (status, _) = lib
        .app
        .owner(
            Method::DELETE,
            &format!("{MOVIES}/{}?deleteFiles=true", movie["id"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!folder.exists());
    assert!(neighbour.exists());
    assert!(lib.dir.path().exists(), "the root folder stays");
}

#[tokio::test]
async fn deleting_with_files_works_when_the_folder_is_already_gone() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let (status, _) = lib
        .app
        .owner(
            Method::DELETE,
            &format!("{MOVIES}/{}?deleteFiles=true", movie["id"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn tags_are_created_renamed_and_deleted() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let app = &lib.app;

    let (status, tag) = app
        .owner(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "label": " 4K-HDR " })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{tag}");
    assert_eq!(tag["label"], "4k-hdr");
    let id = tag["id"].as_i64().unwrap();
    let path = format!("/api/v1/tags/{id}");

    let (status, reply) = app
        .owner(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "label": "4k-hdr" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "label_taken");
    for bad in ["", "has space", "ünï", &"x".repeat(65)] {
        let (status, _) = app
            .owner(Method::POST, "/api/v1/tags", Some(json!({ "label": bad })))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad:?}");
    }

    let (status, renamed) = app
        .owner(Method::PATCH, &path, Some(json!({ "label": "uhd" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["label"], "uhd");
    let (status, _) = app
        .owner(
            Method::PATCH,
            "/api/v1/tags/9999",
            Some(json!({ "label": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, listed) = app.owner(Method::GET, "/api/v1/tags", None).await;
    assert_eq!(listed, json!([{ "id": id, "label": "uhd" }]));

    // A tag on a movie cannot be deleted.
    let movie = add(&lib).await;
    app.owner(
        Method::PATCH,
        &format!("{MOVIES}/{}", movie["id"]),
        Some(json!({ "tags": [id] })),
    )
    .await;
    let (status, reply) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "tag_in_use");
    app.owner(
        Method::PATCH,
        &format!("{MOVIES}/{}", movie["id"]),
        Some(json!({ "tags": [] })),
    )
    .await;
    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn changes_are_audited() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    lib.app
        .owner(
            Method::PATCH,
            &format!("{MOVIES}/{}", movie["id"]),
            Some(json!({ "monitored": false })),
        )
        .await;
    let (_, audit) = lib.app.owner(Method::GET, "/api/v1/audit-log", None).await;
    let text = audit.to_string();
    assert!(
        text.contains("movie.created") && text.contains("movie.updated"),
        "{text}"
    );
}

#[tokio::test]
async fn every_route_needs_its_library_permission() {
    let server = MockServer::start().await;
    tmdb_movie(&server).await;
    let lib = library(&server).await;
    let movie = add(&lib).await;
    let path = format!("{MOVIES}/{}", movie["id"]);
    let profile = movie["profiles"][0]["id"].clone();
    let (_, reader, _) = lib.app.member("read@example.com", &["library:read"]).await;
    let (_, adder, _) = lib.app.member("add@example.com", &["library:add"]).await;
    let (_, editor, _) = lib.app.member("edit@example.com", &["library:edit"]).await;
    let (_, deleter, _) = lib.app.member("del@example.com", &["library:delete"]).await;

    let cases: Vec<(Method, String, Option<Value>, &str, &str)> = vec![
        (Method::GET, MOVIES.into(), None, "library:read", &reader),
        (Method::GET, path.clone(), None, "library:read", &reader),
        (
            Method::GET,
            format!("{path}/files"),
            None,
            "library:read",
            &reader,
        ),
        (
            Method::GET,
            "/api/v1/tags".into(),
            None,
            "library:read",
            &reader,
        ),
        (
            Method::POST,
            MOVIES.into(),
            Some(add_body(&lib)),
            "library:add",
            &adder,
        ),
        (
            Method::POST,
            format!("{path}/profiles"),
            Some(json!({ "qualityProfileId": lib.uhd })),
            "library:add",
            &adder,
        ),
        (
            Method::PATCH,
            path.clone(),
            Some(json!({ "monitored": true })),
            "library:edit",
            &editor,
        ),
        (
            Method::POST,
            "/api/v1/tags".into(),
            Some(json!({ "label": "x" })),
            "library:edit",
            &editor,
        ),
        (
            Method::DELETE,
            format!("{path}/profiles/{profile}"),
            None,
            "library:delete",
            &deleter,
        ),
        (
            Method::DELETE,
            format!("{path}/files/1"),
            None,
            "library:delete",
            &deleter,
        ),
        (
            Method::DELETE,
            path.clone(),
            None,
            "library:delete",
            &deleter,
        ),
    ];
    for (method, uri, body, permission, holder) in cases {
        // The permission holder passes the check (any status but 403).
        let (status, _, _) = lib
            .app
            .send(method.clone(), &uri, Some(holder), body.clone())
            .await;
        assert_ne!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        // Another permission does not.
        let other = if permission == "library:read" {
            &deleter
        } else {
            &reader
        };
        let (status, _, reply) = lib.app.send(method.clone(), &uri, Some(other), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(reply["requiredPermission"], permission, "{method} {uri}");
    }
}
