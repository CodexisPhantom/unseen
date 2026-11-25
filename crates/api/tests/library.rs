use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{method, path, query_param};
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
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        (user_id, cookie.unwrap(), role.id)
    }
}

const MOVIE: &str = include_str!("../../../fixtures/tmdb/movie_603.json");

const LIBRARY: &str = "/api/v1/library";
const KEY: &str = "fixture-key-0123456789abcdef";

fn json_body(body: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

/// The Matrix fixture, changed to another movie.
fn movie_json(id: i64, title: &str, date: &str, imdb: &str) -> String {
    let mut movie: Value = serde_json::from_str(MOVIE).unwrap();
    movie["id"] = json!(id);
    movie["title"] = json!(title);
    movie["original_title"] = json!(title);
    movie["release_date"] = json!(date);
    movie["imdb_id"] = json!(imdb);
    movie["belongs_to_collection"] = Value::Null;
    movie.to_string()
}

fn result(id: i64, title: &str, date: &str) -> Value {
    json!({ "id": id, "title": title, "original_title": title, "overview": "",
            "release_date": date, "poster_path": null })
}

async fn details(server: &MockServer, id: i64, body: String) {
    Mock::given(method("GET"))
        .and(path(format!("/3/movie/{id}")))
        .respond_with(json_body(&body, 200))
        .mount(server)
        .await;
}

async fn search(server: &MockServer, query: &str, results: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path("/3/search/movie"))
        .and(query_param("query", query))
        .respond_with(json_body(
            &json!({ "page": 1, "results": results, "total_pages": 1, "total_results": 0 })
                .to_string(),
            200,
        ))
        .mount(server)
        .await;
}

/// TMDB with The Matrix, Dune (two of that name), Alien and a title that
/// matches nothing.
async fn tmdb(server: &MockServer) {
    details(server, 603, MOVIE.to_owned()).await;
    details(
        server,
        438631,
        movie_json(438631, "Dune", "2021-09-15", "tt1160419"),
    )
    .await;
    details(
        server,
        348,
        movie_json(348, "Alien", "1979-05-25", "tt0078748"),
    )
    .await;
    search(
        server,
        "The Matrix",
        vec![
            result(603, "The Matrix", "1999-03-30"),
            result(604, "The Matrix Reloaded", "2003-05-15"),
        ],
    )
    .await;
    search(
        server,
        "Dune",
        vec![
            result(841, "Dune", "1984-12-14"),
            result(438631, "Dune", "2021-09-15"),
        ],
    )
    .await;
    search(
        server,
        "Mystery Film",
        vec![result(1, "Mystery Film", "2011-01-01")],
    )
    .await;
}

fn write(root: &Path, relative: &str, bytes: usize) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

/// A tree named with Radarr's default templates.
fn tree(root: &Path) {
    write(
        root,
        "The Matrix (1999)/The Matrix (1999) Bluray-1080p.mkv",
        3000,
    );
    write(
        root,
        "The Matrix (1999)/The Matrix (1999) Bluray-720p.mkv",
        1000,
    );
    write(root, "The Matrix (1999)/poster.jpg", 50);
    write(root, "The Matrix (1999)/The Matrix (1999)-sample.mkv", 10);
    write(root, "The Matrix (1999)/Sample/sample.mkv", 10);
    write(root, "The Matrix (1999)/Extras/making of.mkv", 500);
    write(root, "Dune (2021)/Dune (2021) WEBDL-1080p.mkv", 2000);
    write(
        root,
        "Alien (1979) {tmdb-348}/Alien (1979) Bluray-1080p.mkv",
        1500,
    );
    write(root, "Mystery Film (2010)/Mystery Film (2010).mkv", 800);
    write(root, "Empty (2000)/readme.txt", 5);
}

/// Every path under `root` with size and modification time.
fn snapshot(root: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, u64, std::time::SystemTime)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            let name = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string();
            out.push((name, meta.len(), meta.modified().unwrap()));
            if meta.is_dir() {
                walk(&entry.path(), root, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

struct Library {
    app: App,
    root: i64,
    hd: i64,
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
        dir,
    }
}

async fn scan(lib: &Library) -> Value {
    let (status, reply) = lib
        .app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/scan"),
            Some(json!({ "rootFolderId": lib.root })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    reply
}

fn folder<'a>(scan: &'a Value, name: &str) -> &'a Value {
    scan["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["folderName"] == name)
        .unwrap_or_else(|| panic!("{name} is not listed in {scan}"))
}

fn adopt_body(lib: &Library, movies: Value) -> Value {
    json!({ "rootFolderId": lib.root, "qualityProfileIds": [lib.hd], "movies": movies })
}

#[tokio::test]
async fn a_fixture_tree_with_radarr_naming_is_matched_completely() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    tree(lib.dir.path());

    let reply = scan(&lib).await;
    assert_eq!(reply["folders"].as_array().unwrap().len(), 5);

    let matrix = folder(&reply, "The Matrix (1999)");
    assert_eq!(matrix["status"], "matched");
    assert_eq!(matrix["tmdbId"], 603);
    assert_eq!(matrix["title"], "The Matrix");
    assert_eq!(matrix["year"], 1999);
    // Largest first; sample, extras and the poster are not listed.
    let files: Vec<_> = matrix["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["relativePath"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            "The Matrix (1999) Bluray-1080p.mkv",
            "The Matrix (1999) Bluray-720p.mkv"
        ]
    );

    let dune = folder(&reply, "Dune (2021)");
    assert_eq!(
        (dune["status"].as_str(), dune["tmdbId"].as_i64()),
        (Some("matched"), Some(438631))
    );
    // The id tag decides, without a text search.
    let alien = folder(&reply, "Alien (1979) {tmdb-348}");
    assert_eq!(
        (alien["status"].as_str(), alien["tmdbId"].as_i64()),
        (Some("matched"), Some(348))
    );
    assert_eq!(alien["title"], "Alien");
}

#[tokio::test]
async fn a_folder_without_a_confident_match_is_reported_and_not_guessed() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    tree(lib.dir.path());

    let reply = scan(&lib).await;
    let mystery = folder(&reply, "Mystery Film (2010)");
    assert_eq!(mystery["status"], "unmatched");
    assert_eq!(mystery["tmdbId"], Value::Null);
    assert_eq!(folder(&reply, "Empty (2000)")["status"], "noVideo");
}

#[tokio::test]
async fn adoption_changes_nothing_on_disk_and_a_second_scan_lists_nothing_new() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    tree(lib.dir.path());
    let before = snapshot(lib.dir.path());

    let reply = scan(&lib).await;
    let items: Vec<Value> = reply["folders"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["status"] == "matched")
        .map(|f| json!({ "folderName": f["folderName"], "tmdbId": f["tmdbId"] }))
        .collect();
    assert_eq!(items.len(), 3);
    let (status, adopted) = lib
        .app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/adopt"),
            Some(adopt_body(&lib, json!(items))),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{adopted}");
    assert_eq!(adopted["failed"], json!([]));
    assert_eq!(adopted["adopted"].as_array().unwrap().len(), 3);

    assert_eq!(snapshot(lib.dir.path()), before);

    // Again: only the folders that were never adopted are left.
    let again = scan(&lib).await;
    let left: Vec<_> = again["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["folderName"].as_str().unwrap())
        .collect();
    assert_eq!(left, ["Empty (2000)", "Mystery Film (2010)"]);
}

#[tokio::test]
async fn adopted_movies_keep_their_folder_name_and_files() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    tree(lib.dir.path());
    let (status, adopted) = lib
        .app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/adopt"),
            Some(adopt_body(
                &lib,
                json!([
                    { "folderName": "The Matrix (1999)", "tmdbId": 603 },
                    { "folderName": "Alien (1979) {tmdb-348}", "tmdbId": 348 }
                ]),
            )),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{adopted}");
    let matrix_id = adopted["adopted"][0]["movieId"].as_i64().unwrap();
    assert_eq!(adopted["adopted"][0]["fileCount"], 2);

    let (_, movie) = lib
        .app
        .owner(Method::GET, &format!("/api/v1/movies/{matrix_id}"), None)
        .await;
    assert_eq!(movie["folderName"], "The Matrix (1999)");
    assert_eq!(movie["title"], "The Matrix");
    assert_eq!(movie["monitored"], true);
    let (_, alien) = lib.app.owner(Method::GET, "/api/v1/movies", None).await;
    let alien = alien
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["tmdbId"] == 348)
        .unwrap();
    // The folder name is the one on disk, not the template's.
    assert_eq!(alien["folderName"], "Alien (1979) {tmdb-348}");

    let (_, files) = lib
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{matrix_id}/files"),
            None,
        )
        .await;
    let files = files.as_array().unwrap();
    assert_eq!(files.len(), 2);
    let main = files
        .iter()
        .find(|f| f["relativePath"] == "The Matrix (1999) Bluray-1080p.mkv")
        .unwrap();
    assert_eq!(main["managed"], true);
    assert_eq!(main["size"], 3000);
    // Radarr ids: Bluray-1080p is 7, Bluray-720p is 6.
    assert_eq!(main["qualityId"], 7);
    assert_eq!(movie["profiles"][0]["fileId"], main["id"]);
    let other = files
        .iter()
        .find(|f| f["relativePath"] == "The Matrix (1999) Bluray-720p.mkv")
        .unwrap();
    assert_eq!(other["managed"], false);
    assert_eq!(other["qualityId"], 6);
}

#[tokio::test]
async fn a_scan_does_not_list_a_folder_of_a_movie_the_library_has_under_another_name() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    tree(lib.dir.path());
    // Dune is known as `Dune 2021`; the folder `Dune (2021)` is a second
    // folder for the same movie.
    write(lib.dir.path(), "Dune 2021/Dune.mkv", 100);
    let (status, _) = lib
        .app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/adopt"),
            Some(adopt_body(
                &lib,
                json!([{ "folderName": "Dune 2021", "tmdbId": 438631 }]),
            )),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let reply = scan(&lib).await;
    let dune = folder(&reply, "Dune (2021)");
    assert_eq!(dune["status"], "duplicate");
    assert_eq!(dune["tmdbId"], 438631);
}

#[tokio::test]
async fn adopt_reports_each_folder_it_cannot_adopt_and_adopts_the_rest() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    Mock::given(method("GET"))
        .and(path("/3/movie/999999"))
        .respond_with(json_body(
            r#"{"status_code":34,"status_message":"not found"}"#,
            404,
        ))
        .mount(&server)
        .await;
    let lib = library(&server).await;
    tree(lib.dir.path());
    let (status, reply) = lib
        .app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/adopt"),
            Some(adopt_body(
                &lib,
                json!([
                    { "folderName": "Dune (2021)", "tmdbId": 438631 },
                    { "folderName": "../escape", "tmdbId": 603 },
                    { "folderName": "Nowhere (1999)", "tmdbId": 603 },
                    { "folderName": "Empty (2000)", "tmdbId": 603 },
                    { "folderName": "Mystery Film (2010)", "tmdbId": 999999 },
                    { "folderName": "The Matrix (1999)", "tmdbId": 438631 },
                ]),
            )),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["adopted"].as_array().unwrap().len(), 1);
    let codes: Vec<_> = reply["failed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["folderName"].as_str().unwrap(),
                f["code"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            ("../escape", "folder_not_found"),
            ("Nowhere (1999)", "folder_not_found"),
            ("Empty (2000)", "no_video_file"),
            ("Mystery Film (2010)", "tmdb_not_found"),
            ("The Matrix (1999)", "movie_exists"),
        ]
    );
}

#[tokio::test]
async fn bad_requests_are_rejected() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    let (status, _) = lib
        .app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/scan"),
            Some(json!({ "rootFolderId": 999 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for (field, body) in [
        (
            "qualityProfileIds",
            json!({ "rootFolderId": lib.root, "qualityProfileIds": [], "movies": [] }),
        ),
        (
            "minimumAvailability",
            json!({ "rootFolderId": lib.root, "qualityProfileIds": [lib.hd],
                    "minimumAvailability": "never", "movies": [] }),
        ),
        (
            "rootFolderId",
            json!({ "rootFolderId": 999, "qualityProfileIds": [lib.hd], "movies": [] }),
        ),
    ] {
        let (status, reply) = lib
            .app
            .owner(Method::POST, &format!("{LIBRARY}/adopt"), Some(body))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {reply}");
    }
}

#[tokio::test]
async fn scan_without_a_tmdb_key_is_a_409() {
    let server = MockServer::start().await;
    let app = App::new(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, root) = app
        .owner(
            Method::POST,
            "/api/v1/root-folders",
            Some(json!({ "path": dir.path().to_str().unwrap() })),
        )
        .await;
    let (status, reply) = app
        .owner(
            Method::POST,
            &format!("{LIBRARY}/scan"),
            Some(json!({ "rootFolderId": root["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "tmdb_not_configured");
}

#[tokio::test]
async fn scan_and_adopt_need_import_run() {
    let server = MockServer::start().await;
    tmdb(&server).await;
    let lib = library(&server).await;
    let (_, user, _) = lib
        .app
        .member(
            "adder@example.com",
            &["library:add", "library:read", "settings:manage"],
        )
        .await;
    for path in ["scan", "adopt"] {
        let (status, _, reply) = lib
            .app
            .send(
                Method::POST,
                &format!("{LIBRARY}/{path}"),
                Some(&user),
                Some(json!({ "rootFolderId": lib.root })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(reply["requiredPermission"], "import:run");
    }
    let (_, runner, _) = lib.app.member("runner@example.com", &["import:run"]).await;
    let (status, _, _) = lib
        .app
        .send(
            Method::POST,
            &format!("{LIBRARY}/scan"),
            Some(&runner),
            Some(json!({ "rootFolderId": lib.root })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}
