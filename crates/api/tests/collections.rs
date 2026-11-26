use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
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

    /// The cookie of a user who holds a new role with `permissions`.
    async fn member(&self, email: &str, permissions: &[&str]) -> String {
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
            user_id: Set(user_id),
            role_id: Set(role.id),
            all_libraries: Set(true),
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        cookie.unwrap()
    }
}

const MOVIE: &str = include_str!("../../../fixtures/tmdb/movie_603.json");
const COLLECTION: &str = include_str!("../../../fixtures/tmdb/collection_2344.json");

const COLLECTIONS: &str = "/api/v1/collections";
const MOVIES: &str = "/api/v1/movies";
const KEY: &str = "fixture-key-0123456789abcdef";

fn json_body(body: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

/// An app whose TMDB key is set, with a root folder and a profile.
struct Library {
    app: App,
    root: i64,
    hd: i64,
    _dir: TempDir,
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
    }
}

/// TMDB's answer for another movie of the Matrix collection: the fixture of
/// movie 603 with another id, title, date and status.
fn movie_json(id: i64, title: &str, date: &str, status: &str) -> String {
    let mut movie: Value = serde_json::from_str(MOVIE).unwrap();
    movie["id"] = json!(id);
    movie["title"] = json!(title);
    movie["original_title"] = json!(title);
    movie["release_date"] = json!(date);
    movie["status"] = json!(status);
    movie["external_ids"]["imdb_id"] = json!(format!("tt{id}"));
    movie.to_string()
}

async fn tmdb_movie(server: &MockServer, id: i64, body: String) {
    Mock::given(method("GET"))
        .and(path(format!("/3/movie/{id}")))
        .respond_with(json_body(&body, 200))
        .mount(server)
        .await;
}

/// The collection fixture with a third part that TMDB has not announced,
/// and the three movies.
async fn tmdb_collection(server: &MockServer) {
    let mut collection: Value = serde_json::from_str(COLLECTION).unwrap();
    collection["parts"].as_array_mut().unwrap().push(json!({
        "id": 605,
        "title": "The Matrix Resurrections",
        "original_title": "The Matrix Resurrections",
        "overview": "",
        "poster_path": null,
        "release_date": "2031-01-01",
    }));
    Mock::given(method("GET"))
        .and(path("/3/collection/2344"))
        .respond_with(json_body(&collection.to_string(), 200))
        .mount(server)
        .await;
    tmdb_movie(server, 603, MOVIE.to_owned()).await;
    let reloaded = movie_json(604, "The Matrix Reloaded", "2003-05-15", "Released");
    tmdb_movie(server, 604, reloaded).await;
    let unannounced = movie_json(605, "The Matrix Resurrections", "2031-01-01", "");
    tmdb_movie(server, 605, unannounced).await;
}

async fn add_movie(lib: &Library, tmdb_id: i64) -> Value {
    let body = json!({
        "tmdbId": tmdb_id, "rootFolderId": lib.root, "qualityProfileIds": [lib.hd],
    });
    let (status, movie) = lib.app.owner(Method::POST, MOVIES, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{movie}");
    movie
}

#[tokio::test]
async fn adding_movies_of_a_collection_creates_the_collection_once() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let (status, none) = lib.app.owner(Method::GET, COLLECTIONS, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(none, json!([]));

    let first = add_movie(&lib, 603).await;
    let second = add_movie(&lib, 604).await;

    let (_, listed) = lib.app.owner(Method::GET, COLLECTIONS, None).await;
    let rows = listed.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let collection = &rows[0];
    assert_eq!(collection["tmdbId"], 2344);
    assert_eq!(collection["title"], "The Matrix Collection");
    assert_eq!(
        collection["overview"],
        "A series of science fiction action films."
    );
    assert_eq!(collection["monitored"], false);
    assert_eq!(collection["rootFolderId"], Value::Null);
    assert_eq!(collection["minimumAvailability"], "released");
    assert_eq!(collection["movieCount"], 2);
    assert_eq!(first["collectionId"], collection["id"]);
    assert_eq!(second["collectionId"], collection["id"]);
}

#[tokio::test]
async fn the_collection_shows_its_movies_and_the_missing_ones() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;
    let url = format!("{COLLECTIONS}/{}", movie["collectionId"]);

    let (status, detail) = lib.app.owner(Method::GET, &url, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["title"], "The Matrix Collection");
    assert_eq!(detail["movieCount"], 1);
    assert_eq!(detail["movies"].as_array().unwrap().len(), 1);
    assert_eq!(detail["movies"][0]["tmdbId"], 603);
    let missing: Vec<i64> = detail["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["tmdbId"].as_i64().unwrap())
        .collect();
    assert_eq!(missing, [604, 605]);
    assert_eq!(detail["missing"][0]["title"], "The Matrix Reloaded");
    assert_eq!(detail["missing"][0]["year"], 2003);

    let (status, _) = lib
        .app
        .owner(Method::GET, &format!("{COLLECTIONS}/999"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_the_last_movie_keeps_the_collection() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;

    let (status, _) = lib
        .app
        .owner(Method::DELETE, &format!("{MOVIES}/{}", movie["id"]), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, listed) = lib.app.owner(Method::GET, COLLECTIONS, None).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], movie["collectionId"]);
    assert_eq!(listed[0]["movieCount"], 0);
}

#[tokio::test]
async fn monitoring_adds_the_missing_released_movies_with_the_collection_defaults() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;
    let id = movie["collectionId"].clone();
    let url = format!("{COLLECTIONS}/{id}");
    // Two released movies are missing now: 603 and 604. The third part is
    // not announced and stays out.
    lib.app
        .owner(Method::DELETE, &format!("{MOVIES}/{}", movie["id"]), None)
        .await;

    let (status, patched) = lib
        .app
        .owner(
            Method::PATCH,
            &url,
            Some(json!({
                "monitored": true,
                "rootFolderId": lib.root,
                "qualityProfileId": lib.hd,
                "minimumAvailability": "inCinemas",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["monitored"], true);
    assert_eq!(patched["rootFolderId"], lib.root);
    assert_eq!(patched["qualityProfileId"], lib.hd);
    assert_eq!(patched["minimumAvailability"], "inCinemas");
    assert_eq!(patched["movieCount"], 2);

    let (_, movies) = lib.app.owner(Method::GET, MOVIES, None).await;
    let movies = movies.as_array().unwrap();
    assert_eq!(movies.len(), 2);
    for movie in movies {
        assert_eq!(movie["collectionId"], id);
        assert_eq!(movie["monitored"], true);
        assert_eq!(movie["rootFolderId"], lib.root);
        assert_eq!(movie["minimumAvailability"], "inCinemas");
        assert_eq!(movie["profiles"][0]["qualityProfileId"], lib.hd);
        assert_eq!(movie["profiles"][0]["isPrimary"], true);
    }
    let tmdb_ids: Vec<i64> = movies
        .iter()
        .map(|m| m["tmdbId"].as_i64().unwrap())
        .collect();
    assert_eq!(tmdb_ids, [603, 604]);

    // The detail lists only the movie that is still missing.
    let (_, detail) = lib.app.owner(Method::GET, &url, None).await;
    assert_eq!(detail["missing"].as_array().unwrap().len(), 1);
    assert_eq!(detail["missing"][0]["tmdbId"], 605);

    // Monitoring again adds nothing.
    let (status, _) = lib
        .app
        .owner(Method::PATCH, &url, Some(json!({ "monitored": true })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, movies) = lib.app.owner(Method::GET, MOVIES, None).await;
    assert_eq!(movies.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn monitoring_needs_a_root_folder_and_a_profile_and_valid_values() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;
    let url = format!("{COLLECTIONS}/{}", movie["collectionId"]);

    for (field, body) in [
        ("rootFolderId", json!({ "monitored": true })),
        (
            "qualityProfileId",
            json!({ "monitored": true, "rootFolderId": lib.root }),
        ),
        ("rootFolderId", json!({ "rootFolderId": 9999 })),
        ("qualityProfileId", json!({ "qualityProfileId": 9999 })),
        (
            "minimumAvailability",
            json!({ "minimumAvailability": "soon" }),
        ),
    ] {
        let (status, reply) = lib.app.owner(Method::PATCH, &url, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["errors"][0]["field"], field);
    }
    let (_, detail) = lib.app.owner(Method::GET, &url, None).await;
    assert_eq!(detail["monitored"], false);

    // Defaults can be set without monitoring; nothing is added.
    let (status, patched) = lib
        .app
        .owner(
            Method::PATCH,
            &url,
            Some(json!({ "rootFolderId": lib.root, "qualityProfileId": lib.hd })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["monitored"], false);
    let (_, movies) = lib.app.owner(Method::GET, MOVIES, None).await;
    assert_eq!(movies.as_array().unwrap().len(), 1);

    let (status, _) = lib
        .app
        .owner(
            Method::PATCH,
            &format!("{COLLECTIONS}/999"),
            Some(json!({ "monitored": false })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_tmdb_failure_while_monitoring_leaves_the_collection_unchanged() {
    let server = MockServer::start().await;
    tmdb_movie(&server, 603, MOVIE.to_owned()).await;
    Mock::given(method("GET"))
        .and(path("/3/collection/2344"))
        .respond_with(json_body("{}", 500))
        .mount(&server)
        .await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;
    let url = format!("{COLLECTIONS}/{}", movie["collectionId"]);

    let (status, reply) = lib
        .app
        .owner(
            Method::PATCH,
            &url,
            Some(json!({
                "monitored": true, "rootFolderId": lib.root, "qualityProfileId": lib.hd,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{reply}");
    let (_, listed) = lib.app.owner(Method::GET, COLLECTIONS, None).await;
    assert_eq!(listed[0]["monitored"], false);
    assert_eq!(listed[0]["rootFolderId"], Value::Null);
}

#[tokio::test]
async fn changes_are_audited() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;
    let url = format!("{COLLECTIONS}/{}", movie["collectionId"]);
    lib.app
        .owner(
            Method::PATCH,
            &url,
            Some(json!({
                "monitored": true, "rootFolderId": lib.root, "qualityProfileId": lib.hd,
            })),
        )
        .await;

    let (_, audit) = lib.app.owner(Method::GET, "/api/v1/audit-log", None).await;
    let text = audit.to_string();
    assert!(text.contains("collection.updated"), "{text}");
}

#[tokio::test]
async fn reading_needs_library_read_and_changing_needs_library_edit() {
    let server = MockServer::start().await;
    tmdb_collection(&server).await;
    let lib = library(&server).await;
    let movie = add_movie(&lib, 603).await;
    let url = format!("{COLLECTIONS}/{}", movie["collectionId"]);
    let reader = lib.app.member("read@example.com", &["library:read"]).await;
    let editor = lib.app.member("edit@example.com", &["library:edit"]).await;
    let adder = lib.app.member("add@example.com", &["library:add"]).await;

    for (cookie, read, edit) in [
        (&reader, StatusCode::OK, StatusCode::FORBIDDEN),
        (&editor, StatusCode::FORBIDDEN, StatusCode::OK),
        (&adder, StatusCode::FORBIDDEN, StatusCode::FORBIDDEN),
    ] {
        for path in [COLLECTIONS, url.as_str()] {
            let (status, _, _) = lib.app.send(Method::GET, path, Some(cookie), None).await;
            assert_eq!(status, read, "GET {path}");
        }
        let patch = json!({ "monitored": false });
        let (status, _, _) = lib
            .app
            .send(Method::PATCH, &url, Some(cookie), Some(patch))
            .await;
        assert_eq!(status, edit, "PATCH");
    }
}
