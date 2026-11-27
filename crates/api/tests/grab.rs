//! Sending a searched release to a download client (docs/05-api.md,
//! docs/06-state-machines.md).
use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::entities::{history, quality_profiles, root_folders};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use unseen_core::search::{CachedRelease, SearchCache};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";
const GRAB: &str = "/api/v1/releases/grab";
const CATEGORY: &str = "unseen";
const HASH: &str = "aaaabbbbccccddddeeeeffff0000111122223333";

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

impl App {
    async fn new(cache: SearchCache) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let mut state = AppState::new(db, "test secret", None).await.unwrap();
        state.search_cache = cache;
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

    /// A monitored movie with one profile, so a grab has a profile to fill.
    async fn seed_movie(&self) -> (i32, i32) {
        let folder = root_folders::ActiveModel {
            path: Set("/movies".into()),
            created_at: Set(db::now()),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        let profile = quality_profiles::Entity::find()
            .one(&self.state.db)
            .await
            .unwrap()
            .unwrap();
        let movie = db::movies::create(
            &self.state.db,
            db::movies::NewMovie {
                metadata: db::movies::Metadata {
                    tmdb_id: 603,
                    imdb_id: Some("tt0133093".into()),
                    title: "The Matrix".into(),
                    original_title: None,
                    sort_title: "matrix".into(),
                    year: 1999,
                    overview: None,
                    runtime: Some(136),
                    status: "released".into(),
                    in_cinemas: None,
                    digital_release: None,
                    physical_release: None,
                    poster_path: None,
                },
                collection_id: None,
                root_folder_id: folder.id,
                folder_name: "The Matrix (1999)".into(),
                monitored: true,
                minimum_availability: "released".into(),
            },
            &[profile.id],
            &[],
        )
        .await
        .unwrap();
        (movie.model.id, movie.profiles[0].id)
    }

    async fn add_client(&self, server: &MockServer) -> i64 {
        let (status, reply) = self
            .owner(
                Method::POST,
                "/api/v1/download-clients",
                Some(json!({
                    "name": "Local",
                    "url": server.uri(),
                    "password": "good",
                    "category": CATEGORY,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        reply["id"].as_i64().unwrap()
    }
}

/// The calls qBittorrent needs before it accepts a magnet link.
async fn mount_qbittorrent(server: &MockServer, add_status: u16, adds: u64) {
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "SID=abc; path=/")
                .set_body_string("Ok."),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/categories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            CATEGORY: { "name": CATEGORY, "savePath": "/downloads" },
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/add"))
        .and(body_string_contains(CATEGORY))
        .respond_with(
            ResponseTemplate::new(add_status).set_body_string(if add_status == 200 {
                "Ok."
            } else {
                "Fails."
            }),
        )
        .expect(adds)
        .mount(server)
        .await;
}

fn cached_release(movie_id: i32, movie_profile_id: i32, accepted: bool) -> CachedRelease {
    CachedRelease {
        movie_id,
        movie_profile_id,
        indexer_id: 1,
        indexer_name: "Alpha".to_owned(),
        guid: "guid:matrix".to_owned(),
        title: "The.Matrix.1999.1080p.BluRay".to_owned(),
        download_url: None,
        magnet_url: Some(format!(
            "magnet:?xt=urn:btih:{HASH}&dn=The.Matrix.1999.1080p.BluRay"
        )),
        info_hash: Some(HASH.to_owned()),
        size: Some(4_000_000_000),
        quality_id: 7,
        custom_format_score: 0,
        protocol: "torrent".to_owned(),
        accepted,
    }
}

#[tokio::test]
async fn grab_sends_the_release_with_the_client_category_and_records_it() {
    let server = MockServer::start().await;
    mount_qbittorrent(&server, 200, 1).await;
    let app = App::new(SearchCache::new()).await;
    let (movie_id, movie_profile_id) = app.seed_movie().await;
    let client_id = app.add_client(&server).await;
    app.state.search_cache.insert(
        "rel1".into(),
        cached_release(movie_id, movie_profile_id, true),
    );
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel1" })))
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["downloadClientId"], client_id);
    assert_eq!(reply["clientDownloadId"], HASH);
    let download = db::downloads::find(&app.state.db, reply["downloadId"].as_i64().unwrap() as i32)
        .await
        .unwrap()
        .expect("the download row exists");
    assert_eq!(download.state, "queued");
    assert_eq!(download.client_download_id, HASH);
    assert_eq!(download.movie_profile_id, Some(movie_profile_id));
    assert_eq!(download.release_title, "The.Matrix.1999.1080p.BluRay");
    let entries = history::Entity::find().all(&app.state.db).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].event, "grabbed");
    assert_eq!(entries[0].movie_id, Some(movie_id));
    // The id is consumed: a grab is not repeated by accident.
    assert!(app.state.search_cache.get("rel1").is_none());
}

#[tokio::test]
async fn a_second_grab_of_the_profile_is_refused() {
    let server = MockServer::start().await;
    mount_qbittorrent(&server, 200, 1).await;
    let app = App::new(SearchCache::new()).await;
    let (movie_id, movie_profile_id) = app.seed_movie().await;
    app.add_client(&server).await;
    app.state.search_cache.insert(
        "rel1".into(),
        cached_release(movie_id, movie_profile_id, true),
    );
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel1" })))
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    app.state.search_cache.insert(
        "rel2".into(),
        cached_release(movie_id, movie_profile_id, true),
    );
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel2" })))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "download_exists");
}

#[tokio::test]
async fn a_client_failure_is_a_bad_gateway_and_leaves_no_download() {
    let server = MockServer::start().await;
    mount_qbittorrent(&server, 500, 2).await;
    let app = App::new(SearchCache::new()).await;
    let (movie_id, movie_profile_id) = app.seed_movie().await;
    app.add_client(&server).await;
    app.state.search_cache.insert(
        "rel1".into(),
        cached_release(movie_id, movie_profile_id, true),
    );
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel1" })))
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{reply}");
    // The id survives the failure, so the user can retry the same release.
    assert!(app.state.search_cache.get("rel1").is_some());
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel1" })))
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{reply}");
    assert!(app.state.search_cache.get("rel1").is_some());
    assert!(
        db::downloads::list_active(&app.state.db)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        history::Entity::find()
            .all(&app.state.db)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_unknown_or_rejected_release_is_refused() {
    let app = App::new(SearchCache::new()).await;
    let (movie_id, movie_profile_id) = app.seed_movie().await;
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "gone" })))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
    app.state.search_cache.insert(
        "rel1".into(),
        cached_release(movie_id, movie_profile_id, false),
    );
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel1" })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
}

/// Two grabs of one movie profile run at the same time: the second waits for
/// the first, sees its download, and is refused, so the client is asked once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_grabs_of_one_profile_add_once() {
    let server = MockServer::start().await;
    mount_qbittorrent(&server, 200, 1).await;
    let app = App::new(SearchCache::new()).await;
    let (movie_id, movie_profile_id) = app.seed_movie().await;
    app.add_client(&server).await;
    app.state.search_cache.insert(
        "rel1".into(),
        cached_release(movie_id, movie_profile_id, true),
    );
    app.state.search_cache.insert(
        "rel2".into(),
        cached_release(movie_id, movie_profile_id, true),
    );
    let (first, second) = tokio::join!(
        app.owner(Method::POST, GRAB, Some(json!({ "id": "rel1" }))),
        app.owner(Method::POST, GRAB, Some(json!({ "id": "rel2" }))),
    );
    let statuses = [first.0, second.0];
    let ok = statuses
        .iter()
        .filter(|status| **status == StatusCode::OK)
        .count();
    let conflict = statuses
        .iter()
        .filter(|status| **status == StatusCode::CONFLICT)
        .count();
    assert_eq!((ok, conflict), (1, 1), "{first:?} {second:?}");
    assert_eq!(
        db::downloads::list_active(&app.state.db)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// The client takes the release, then the download row cannot be stored: the
/// release is taken back from the client instead of piling up there.
#[tokio::test]
async fn a_failed_download_insert_takes_the_release_back() {
    let server = MockServer::start().await;
    mount_qbittorrent(&server, 200, 1).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "hash": HASH,
            "name": "The.Matrix.1999.1080p.BluRay",
            "category": CATEGORY,
        }])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/delete"))
        .and(body_string_contains(HASH))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let app = App::new(SearchCache::new()).await;
    let (movie_id, _) = app.seed_movie().await;
    app.add_client(&server).await;
    // A movie profile id that does not exist makes the insert fail after the
    // client accepted the release.
    app.state
        .search_cache
        .insert("rel1".into(), cached_release(movie_id, 9999, true));
    let (status, reply) = app
        .owner(Method::POST, GRAB, Some(json!({ "id": "rel1" })))
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{reply}");
    assert!(
        db::downloads::list_active(&app.state.db)
            .await
            .unwrap()
            .is_empty()
    );
}
