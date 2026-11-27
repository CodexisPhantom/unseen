//! Integration tests for `GET /api/v1/queue` and `DELETE /api/v1/queue/{id}` (issue #63).

use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::downloads::NewDownload;
use db::entities::{quality_profiles, role_permissions, roles, root_folders, user_roles};
use db::movies::{Metadata, NewMovie};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "unseenarr.test";
const ORIGIN: &str = "http://unseenarr.test";
const PASSWORD: &str = "correct horse";
const TORRENT_HASH: &str = "8c212779b4abde7c6bc608063a0d008b7e40ce32";

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
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
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

    async fn owner(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (status, _, body) = self.send(method, uri, Some(&self.owner), body).await;
        (status, body)
    }

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

    async fn seed_movie(&self) -> i32 {
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
        db::movies::create(
            &self.state.db,
            NewMovie {
                metadata: Metadata {
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
        .unwrap()
        .model
        .id
    }

    async fn create_qb_client(&self, base_url: &str) -> i32 {
        let (status, body) = self
            .owner(
                Method::POST,
                "/api/v1/download-clients",
                Some(json!({
                    "name": "qBit",
                    "kind": "qbittorrent",
                    "url": base_url,
                    "username": "admin",
                    "password": "pw",
                    "category": "radarr",
                    "enabled": true,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["id"].as_i64().unwrap() as i32
    }
}

async fn mount_qb_login(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "SID=abc123; HttpOnly; path=/")
                .set_body_string("Ok."),
        )
        .mount(server)
        .await;
}

async fn mount_qb_torrents(
    server: &MockServer,
    progress: f64,
    dlspeed: i64,
    eta: i64,
    state: &str,
) {
    let payload = json!([
        {
            "hash": TORRENT_HASH,
            "name": "The.Matrix.1999.1080p.BluRay",
            "category": "radarr",
            "state": state,
            "progress": progress,
            "size": 10_000_000_000_i64,
            "amount_left": 5_000_000_000_i64,
            "dlspeed": dlspeed,
            "eta": eta
        }
    ]);
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/info"))
        .and(query_param("category", "radarr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payload))
        .mount(server)
        .await;
}

#[tokio::test]
async fn queue_enforces_permissions() {
    let app = App::new().await;
    let no_perms = app.member("none@example.com", &[]).await;
    let reader = app.member("reader@example.com", &["queue:read"]).await;

    let (status, _, _) = app.send(Method::GET, "/api/v1/queue", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _, body) = app
        .send(Method::GET, "/api/v1/queue", Some(&no_perms), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["requiredPermission"], "queue:read");

    let (status, _, body) = app
        .send(Method::GET, "/api/v1/queue", Some(&reader), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!([]));

    let (status, _, body) = app
        .send(Method::DELETE, "/api/v1/queue/1", Some(&reader), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["requiredPermission"], "queue:manage");
}

#[tokio::test]
async fn list_queue_merges_live_qbittorrent_progress() {
    let app = App::new().await;
    let server = MockServer::start().await;
    mount_qb_login(&server).await;
    mount_qb_torrents(&server, 0.42, 12_500_000, 320, "downloading").await;

    let movie_id = app.seed_movie().await;
    let client_id = app.create_qb_client(&server.uri()).await;

    let download = db::downloads::create(
        &app.state.db,
        NewDownload {
            movie_id,
            movie_profile_id: None,
            indexer_id: None,
            download_client_id: Some(client_id),
            client_download_id: TORRENT_HASH.into(),
            release_title: "The.Matrix.1999.1080p.BluRay".into(),
            quality_id: 7,
            custom_format_score: 0,
            size: 10_000_000_000,
            state: "queued".into(),
        },
    )
    .await
    .unwrap();

    let (status, body) = app.owner(Method::GET, "/api/v1/queue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], download.id);
    assert_eq!(items[0]["movieTitle"], "The Matrix");
    assert_eq!(items[0]["movieYear"], 1999);
    assert_eq!(items[0]["downloadClientName"], "qBit");
    assert_eq!(items[0]["progress"], 0.42);
    assert_eq!(items[0]["speed"], 12_500_000);
    assert_eq!(items[0]["eta"], 320);
    assert_eq!(items[0]["state"], "downloading");
}

#[tokio::test]
async fn delete_queue_item_removes_from_mocked_qbittorrent_and_blocklists() {
    let app = App::new().await;
    let server = MockServer::start().await;
    mount_qb_login(&server).await;
    mount_qb_torrents(&server, 0.5, 1_000_000, 120, "downloading").await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/delete"))
        .and(body_string_contains(TORRENT_HASH))
        .and(body_string_contains("deleteFiles=true"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let movie_id = app.seed_movie().await;
    let client_id = app.create_qb_client(&server.uri()).await;

    let download = db::downloads::create(
        &app.state.db,
        NewDownload {
            movie_id,
            movie_profile_id: None,
            indexer_id: None,
            download_client_id: Some(client_id),
            client_download_id: TORRENT_HASH.into(),
            release_title: "The.Matrix.1999.1080p.BluRay".into(),
            quality_id: 7,
            custom_format_score: 0,
            size: 10_000_000_000,
            state: "downloading".into(),
        },
    )
    .await
    .unwrap();

    let mut rx = app.state.events.subscribe();

    let (status, body) = app
        .owner(
            Method::DELETE,
            &format!(
                "/api/v1/queue/{}?removeFromClient=true&blocklist=true",
                download.id
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // The queue is now empty.
    let (status, body) = app.owner(Method::GET, "/api/v1/queue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!([]));

    // The release was added to the blocklist.
    let blocked = db::downloads::list_blocklist_for_movie(&app.state.db, movie_id)
        .await
        .unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].release_title, "The.Matrix.1999.1080p.BluRay");

    // An SSE `download.state` event was published.
    let event = rx.try_recv().expect("expected download.state event");
    assert_eq!(event.kind, "download.state");
    assert_eq!(
        event.data,
        json!({
            "id": download.id,
            "oldState": "downloading",
            "newState": "removed",
        })
    );

    // Deleting the same item again returns 404.
    let (status, _) = app
        .owner(
            Method::DELETE,
            &format!("/api/v1/queue/{}", download.id),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_queue_item_can_skip_client_and_blocklist() {
    let app = App::new().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/delete"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let movie_id = app.seed_movie().await;
    let client_id = app.create_qb_client(&server.uri()).await;

    let download = db::downloads::create(
        &app.state.db,
        NewDownload {
            movie_id,
            movie_profile_id: None,
            indexer_id: None,
            download_client_id: Some(client_id),
            client_download_id: TORRENT_HASH.into(),
            release_title: "The.Matrix.1999.1080p.BluRay".into(),
            quality_id: 7,
            custom_format_score: 0,
            size: 10_000_000_000,
            state: "downloading".into(),
        },
    )
    .await
    .unwrap();

    let (status, body) = app
        .owner(
            Method::DELETE,
            &format!(
                "/api/v1/queue/{}?removeFromClient=false&blocklist=false",
                download.id
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let blocked = db::downloads::list_blocklist_for_movie(&app.state.db, movie_id)
        .await
        .unwrap();
    assert!(blocked.is_empty());
}
