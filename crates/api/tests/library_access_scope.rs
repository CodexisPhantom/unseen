//! Integration tests for per-library access scoping (issue #138, ADR 0012).

use std::time::Duration;

use api::{AppState, router};
use axum::Router;
use axum::body::{Body, BodyDataStream, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::downloads::NewDownload;
use db::entities::{collections, movie_files, quality_profiles};
use db::movies::{Metadata, NewMovie};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::time::timeout;
use tower::ServiceExt;

const HOST: &str = "unseenarr.test";
const ORIGIN: &str = "http://unseenarr.test";
const PASSWORD: &str = "correct horse";
const EVENT_TIMEOUT: Duration = Duration::from_secs(2);
const SILENCE_WINDOW: Duration = Duration::from_millis(120);

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
            .send(Method::POST, "/api/v1/setup", None, None, Some(body))
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
        api_key: Option<&str>,
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
        if let Some(key) = api_key {
            request = request.header("X-Api-Key", key);
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
        let (status, _, body) = self.send(method, path, Some(&self.owner), None, body).await;
        (status, body)
    }

    async fn create_role(&self, name: &str, permissions: &[&str]) -> i32 {
        let (status, body) = self
            .owner(
                Method::POST,
                "/api/v1/roles",
                Some(json!({ "name": name, "permissions": permissions })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["id"].as_i64().unwrap() as i32
    }

    async fn create_user(&self, email: &str, role_assignments: Value) -> (String, String) {
        let (status, created) = self
            .owner(
                Method::POST,
                "/api/v1/users",
                Some(json!({
                    "name": email,
                    "email": email,
                    "password": PASSWORD,
                    "roleAssignments": role_assignments,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let id = created["id"].as_str().unwrap().to_owned();
        let (status, cookie, reply) = self
            .send(
                Method::POST,
                "/api/auth/sign-in/email",
                None,
                None,
                Some(json!({ "email": email, "password": PASSWORD })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        (id, cookie.unwrap())
    }

    async fn seed_profile(&self) -> i32 {
        quality_profiles::ActiveModel {
            name: Set(format!("Profile {}", rand::random::<u32>())),
            upgrade_allowed: Set(true),
            cutoff_quality_id: Set(7),
            items: Set(json!([])),
            language: Set("any".into()),
            min_format_score: Set(0),
            cutoff_format_score: Set(0),
            min_upgrade_format_score: Set(1),
            created_at: Set(db::now()),
            updated_at: Set(db::now()),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .unwrap()
        .id
    }

    async fn seed_movie(
        &self,
        tmdb_id: i32,
        title: &str,
        root_folder_id: i32,
        profile_id: i32,
        collection_id: Option<i32>,
    ) -> (i32, i32, i32) {
        let movie = db::movies::create(
            &self.state.db,
            NewMovie {
                metadata: Metadata {
                    tmdb_id,
                    imdb_id: None,
                    title: title.into(),
                    original_title: None,
                    sort_title: title.to_lowercase(),
                    year: 2024,
                    overview: None,
                    runtime: Some(100),
                    status: "released".into(),
                    in_cinemas: None,
                    digital_release: None,
                    physical_release: None,
                    poster_path: None,
                },
                root_folder_id,
                folder_name: format!("{title} (2024)"),
                monitored: true,
                minimum_availability: "released".into(),
                collection_id,
            },
            &[profile_id],
            &[],
        )
        .await
        .unwrap();
        let file = movie_files::ActiveModel {
            movie_id: Set(movie.model.id),
            relative_path: Set(format!("{title}.mkv")),
            size: Set(1024),
            quality_id: Set(7),
            movie_profile_id: Set(Some(movie.profiles[0].id)),
            added_at: Set(db::now()),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        (movie.model.id, movie.profiles[0].id, file.id)
    }

    async fn subscribe(&self, cookie: &str) -> SseStream {
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/events")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        SseStream::new(response.into_body())
    }
}

struct SseStream {
    body: BodyDataStream,
    buffer: String,
}

impl SseStream {
    fn new(body: Body) -> Self {
        Self {
            body: body.into_data_stream(),
            buffer: String::new(),
        }
    }

    async fn next_event(&mut self) -> Option<(String, Value)> {
        loop {
            while let Some(split) = self.buffer.find("\n\n") {
                let raw = self.buffer[..split].to_owned();
                self.buffer.drain(..split + 2);
                let mut name = None;
                let mut data = Vec::new();
                for line in raw.lines() {
                    if let Some(rest) = line.strip_prefix("event:") {
                        name = Some(rest.trim().to_owned());
                    } else if let Some(rest) = line.strip_prefix("data:") {
                        data.push(rest.trim_start().to_owned());
                    }
                }
                if let Some(name) = name {
                    let parsed = serde_json::from_str(&data.join("\n")).unwrap_or(Value::Null);
                    return Some((name, parsed));
                }
            }
            let chunk = self.body.next().await?.expect("valid SSE chunk");
            self.buffer
                .push_str(&std::str::from_utf8(&chunk).unwrap().replace("\r\n", "\n"));
        }
    }

    async fn expect_event(&mut self) -> (String, Value) {
        timeout(EVENT_TIMEOUT, self.next_event())
            .await
            .expect("timed out waiting for SSE event")
            .expect("SSE stream ended early")
    }

    async fn assert_no_event(&mut self) {
        if let Ok(Some((name, data))) = timeout(SILENCE_WINDOW, self.next_event()).await {
            panic!("expected no SSE event, but got `{name}`: {data}");
        }
    }
}

#[tokio::test]
async fn scoped_user_cannot_see_or_mutate_items_of_another_library() {
    let app = App::new().await;
    let default_lib_id = db::libraries::default_id(&app.state.db)
        .await
        .unwrap()
        .unwrap();

    // Create a second library ("Kids") and two real directories as root folders.
    let (status, kids_lib) = app
        .owner(
            Method::POST,
            "/api/v1/libraries",
            Some(json!({ "name": "Kids" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{kids_lib}");
    let kids_lib_id = kids_lib["id"].as_i64().unwrap() as i32;

    let adult_dir = app._dir.path().join("adult");
    let kids_dir = app._dir.path().join("kids");
    std::fs::create_dir_all(&adult_dir).unwrap();
    std::fs::create_dir_all(&kids_dir).unwrap();

    let (status, adult_rf) = app
        .owner(
            Method::POST,
            "/api/v1/root-folders",
            Some(json!({ "path": adult_dir.to_str().unwrap() })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{adult_rf}");
    assert_eq!(adult_rf["libraryId"], default_lib_id);
    let adult_rf_id = adult_rf["id"].as_i64().unwrap() as i32;

    let (status, kids_rf) = app
        .owner(
            Method::POST,
            "/api/v1/root-folders",
            Some(json!({ "path": kids_dir.to_str().unwrap(), "libraryId": kids_lib_id })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{kids_rf}");
    assert_eq!(kids_rf["libraryId"], kids_lib_id);
    let kids_rf_id = kids_rf["id"].as_i64().unwrap() as i32;

    // Seed two collections: one with only an adult movie, one spanning both libraries.
    let col_adult = collections::ActiveModel {
        tmdb_id: Set(9001),
        title: Set("Adult Saga".into()),
        sort_title: Set("adult saga".into()),
        monitored: Set(false),
        root_folder_id: Set(Some(adult_rf_id)),
        minimum_availability: Set("released".into()),
        added_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    let col_mixed = collections::ActiveModel {
        tmdb_id: Set(9002),
        title: Set("Mixed Saga".into()),
        sort_title: Set("mixed saga".into()),
        monitored: Set(false),
        root_folder_id: Set(Some(adult_rf_id)),
        minimum_availability: Set("released".into()),
        added_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let profile_id = app.seed_profile().await;
    let (adult_movie_id, adult_mp_id, adult_file_id) = app
        .seed_movie(
            101,
            "Adult Movie",
            adult_rf_id,
            profile_id,
            Some(col_adult.id),
        )
        .await;
    let (mixed_adult_id, _, _) = app
        .seed_movie(
            102,
            "Mixed Adult",
            adult_rf_id,
            profile_id,
            Some(col_mixed.id),
        )
        .await;
    let (kids_movie_id, _, _) = app
        .seed_movie(
            201,
            "Kids Movie",
            kids_rf_id,
            profile_id,
            Some(col_mixed.id),
        )
        .await;

    // Seed active downloads for both libraries.
    let adult_dl = db::downloads::create(
        &app.state.db,
        NewDownload {
            movie_id: adult_movie_id,
            movie_profile_id: Some(adult_mp_id),
            indexer_id: None,
            download_client_id: None,
            client_download_id: "hash-adult".into(),
            release_title: "Adult.Movie.2024.1080p".into(),
            quality_id: 7,
            custom_format_score: 0,
            size: 1000,
            state: "downloading".into(),
        },
    )
    .await
    .unwrap();
    let kids_dl = db::downloads::create(
        &app.state.db,
        NewDownload {
            movie_id: kids_movie_id,
            movie_profile_id: None,
            indexer_id: None,
            download_client_id: None,
            client_download_id: "hash-kids".into(),
            release_title: "Kids.Movie.2024.1080p".into(),
            quality_id: 7,
            custom_format_score: 0,
            size: 1000,
            state: "downloading".into(),
        },
    )
    .await
    .unwrap();

    // Create a role with broad permissions, assigned ONLY to the Kids library.
    let role_id = app
        .create_role(
            "Kids Manager",
            &[
                "library:read",
                "library:add",
                "library:edit",
                "library:delete",
                "search:run",
                "queue:read",
                "queue:manage",
                "import:run",
                "subtitles:manage",
                "settings:read",
                "settings:manage",
            ],
        )
        .await;
    let (_kid_id, kid_cookie) = app
        .create_user(
            "kid@example.com",
            json!([{ "roleId": role_id, "libraryIds": [kids_lib_id] }]),
        )
        .await;

    // 1. Lists are filtered to Kids only.
    let (status, _, movies) = app
        .send(Method::GET, "/api/v1/movies", Some(&kid_cookie), None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let movie_ids: Vec<i64> = movies
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_i64().unwrap())
        .collect();
    assert_eq!(movie_ids, vec![i64::from(kids_movie_id)]);

    let (status, _, cols) = app
        .send(
            Method::GET,
            "/api/v1/collections",
            Some(&kid_cookie),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let cols = cols.as_array().unwrap();
    assert_eq!(cols.len(), 1);
    assert_eq!(cols[0]["id"], col_mixed.id);
    assert_eq!(cols[0]["movieCount"], 1, "must count only Kids movies");
    assert!(
        cols[0]["rootFolderId"].is_null(),
        "must not leak another library's root folder id"
    );

    let (status, _, queue) = app
        .send(Method::GET, "/api/v1/queue", Some(&kid_cookie), None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let queue_ids: Vec<i64> = queue
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["id"].as_i64().unwrap())
        .collect();
    assert_eq!(queue_ids, vec![i64::from(kids_dl.id)]);

    let (status, _, libs) = app
        .send(
            Method::GET,
            "/api/v1/libraries",
            Some(&kid_cookie),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(libs.as_array().unwrap().len(), 1);
    assert_eq!(libs[0]["id"], kids_lib_id);

    let (status, _, rfs) = app
        .send(
            Method::GET,
            "/api/v1/root-folders",
            Some(&kid_cookie),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rfs.as_array().unwrap().len(), 1);
    assert_eq!(rfs[0]["id"], kids_rf_id);

    // 2. Direct access to items outside Kids returns 404 (never 403).
    let denied_calls = [
        (
            Method::GET,
            format!("/api/v1/movies/{adult_movie_id}"),
            None,
        ),
        (
            Method::PATCH,
            format!("/api/v1/movies/{adult_movie_id}"),
            Some(json!({ "monitored": false })),
        ),
        (
            Method::DELETE,
            format!("/api/v1/movies/{adult_movie_id}"),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/movies/{adult_movie_id}/profiles"),
            Some(json!({ "qualityProfileId": profile_id })),
        ),
        (
            Method::DELETE,
            format!("/api/v1/movies/{adult_movie_id}/profiles/{adult_mp_id}"),
            None,
        ),
        (
            Method::GET,
            format!("/api/v1/movies/{adult_movie_id}/files"),
            None,
        ),
        (
            Method::DELETE,
            format!("/api/v1/movies/{adult_movie_id}/files/{adult_file_id}"),
            None,
        ),
        (
            Method::GET,
            format!("/api/v1/movies/{adult_movie_id}/releases"),
            None,
        ),
        (
            Method::GET,
            format!("/api/v1/movies/{adult_movie_id}/subtitles"),
            None,
        ),
        (
            Method::PUT,
            format!("/api/v1/movies/{adult_movie_id}/language-profile"),
            Some(json!({ "languageProfileId": null })),
        ),
        (
            Method::POST,
            format!("/api/v1/movies/{adult_movie_id}/subtitles/scan"),
            None,
        ),
        (
            Method::POST,
            "/api/v1/movies".to_owned(),
            Some(json!({
                "tmdbId": 99999,
                "rootFolderId": adult_rf_id,
                "qualityProfileIds": [profile_id],
            })),
        ),
        (
            Method::POST,
            "/api/v1/library/scan".to_owned(),
            Some(json!({ "rootFolderId": adult_rf_id })),
        ),
        (
            Method::POST,
            "/api/v1/library/adopt".to_owned(),
            Some(json!({
                "rootFolderId": adult_rf_id,
                "qualityProfileIds": [profile_id],
                "movies": [],
            })),
        ),
        (
            Method::GET,
            format!("/api/v1/collections/{}", col_adult.id),
            None,
        ),
        (
            Method::PATCH,
            format!("/api/v1/collections/{}", col_adult.id),
            Some(json!({ "monitored": false })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/collections/{}", col_mixed.id),
            Some(json!({ "rootFolderId": adult_rf_id })),
        ),
        (
            Method::DELETE,
            format!("/api/v1/queue/{}", adult_dl.id),
            None,
        ),
        (
            Method::PATCH,
            format!("/api/v1/libraries/{default_lib_id}"),
            Some(json!({ "name": "Hacked" })),
        ),
        (
            Method::DELETE,
            format!("/api/v1/libraries/{default_lib_id}"),
            None,
        ),
        (
            Method::DELETE,
            format!("/api/v1/root-folders/{adult_rf_id}"),
            None,
        ),
        (
            Method::PATCH,
            format!("/api/v1/root-folders/{kids_rf_id}"),
            Some(json!({ "libraryId": default_lib_id })),
        ),
    ];
    for (method, path, body) in denied_calls {
        let (status, _, reply) = app
            .send(method.clone(), &path, Some(&kid_cookie), None, body)
            .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} should return 404, got {status}: {reply}"
        );
    }

    // Items in Kids are still accessible.
    let (status, _, _) = app
        .send(
            Method::GET,
            &format!("/api/v1/movies/{kids_movie_id}"),
            Some(&kid_cookie),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app
        .send(
            Method::DELETE,
            &format!("/api/v1/queue/{}", kids_dl.id),
            Some(&kid_cookie),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Adult movie and adult download are still intact.
    assert!(
        db::movies::find(&app.state.db, adult_movie_id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        db::movies::find(&app.state.db, mixed_adult_id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        db::downloads::find(&app.state.db, adult_dl.id)
            .await
            .unwrap()
            .unwrap()
            .state,
        "downloading"
    );

    // 3. SSE events are filtered by library.
    let mut kid_stream = app.subscribe(&kid_cookie).await;
    let mut owner_stream = app.subscribe(&app.owner).await;

    // Mutating an Adult movie notifies Owner, NOT kid.
    let (status, _) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/movies/{adult_movie_id}"),
            Some(json!({ "monitored": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.state
        .publish_download_progress_in_library(adult_dl.id, 0.5, default_lib_id);
    assert_eq!(owner_stream.expect_event().await.0, "movie.updated");
    assert_eq!(owner_stream.expect_event().await.0, "download.progress");
    kid_stream.assert_no_event().await;

    // Mutating a Kids movie notifies both Owner and kid.
    let (status, _) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/movies/{kids_movie_id}"),
            Some(json!({ "monitored": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kid_stream.expect_event().await,
        ("movie.updated".into(), json!({ "id": kids_movie_id }))
    );

    // 4. Radarr & Sonarr compat APIs inherit the user's library scope via API key.
    let (status, _, key_resp) = app
        .send(
            Method::POST,
            "/api/v1/me/api-keys",
            Some(&kid_cookie),
            None,
            Some(json!({
                "name": "Kids Key",
                "permissions": ["library:read", "settings:read"],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{key_resp}");
    let api_key = key_resp["key"].as_str().unwrap().to_owned();

    let (status, _, radarr_movies) = app
        .send(
            Method::GET,
            "/radarr/api/v3/movie",
            None,
            Some(&api_key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(radarr_movies.as_array().unwrap().len(), 1);
    assert_eq!(radarr_movies[0]["id"], kids_movie_id);

    let (status, _, radarr_by_tmdb) = app
        .send(
            Method::GET,
            "/radarr/api/v3/movie?tmdbId=101",
            None,
            Some(&api_key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(radarr_by_tmdb.as_array().unwrap().is_empty());

    let (status, _, _) = app
        .send(
            Method::GET,
            &format!("/radarr/api/v3/movie/{adult_movie_id}"),
            None,
            Some(&api_key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, radarr_rfs) = app
        .send(
            Method::GET,
            "/radarr/api/v3/rootfolder",
            None,
            Some(&api_key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(radarr_rfs.as_array().unwrap().len(), 1);
    assert_eq!(radarr_rfs[0]["id"], kids_rf_id);

    let (status, _, sonarr_rfs) = app
        .send(
            Method::GET,
            "/sonarr/api/v3/rootfolder",
            None,
            Some(&api_key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sonarr_rfs.as_array().unwrap().len(), 1);
    assert_eq!(sonarr_rfs[0]["id"], kids_rf_id);
}

#[tokio::test]
async fn scoped_escalation_and_library_deletion_cannot_widen_access() {
    let app = App::new().await;
    let default_lib_id = db::libraries::default_id(&app.state.db)
        .await
        .unwrap()
        .unwrap();
    let (status, kids_lib) = app
        .owner(
            Method::POST,
            "/api/v1/libraries",
            Some(json!({ "name": "Kids" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let kids_lib_id = kids_lib["id"].as_i64().unwrap() as i32;

    let manager_role = app
        .create_role("Kids Admin", &["users:manage", "library:read"])
        .await;
    let reader_role = app.create_role("Reader", &["library:read"]).await;
    let (_mgr_id, mgr_cookie) = app
        .create_user(
            "mgr@example.com",
            json!([{ "roleId": manager_role, "libraryIds": [kids_lib_id] }]),
        )
        .await;
    let (target_id, target_cookie) = app.create_user("target@example.com", json!([])).await;

    // Manager scoped to Kids CANNOT assign Reader to Default or All libraries.
    for attempt in [
        json!({ "roleAssignments": [{ "roleId": reader_role, "libraryIds": null }] }),
        json!({ "roleAssignments": [{ "roleId": reader_role, "libraryIds": [default_lib_id] }] }),
        json!({ "roleAssignments": [{ "roleId": reader_role, "libraryIds": [default_lib_id, kids_lib_id] }] }),
    ] {
        let (status, _, reply) = app
            .send(
                Method::PATCH,
                &format!("/api/v1/users/{target_id}"),
                Some(&mgr_cookie),
                None,
                Some(attempt),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    }

    // Manager scoped to Kids CAN assign Reader scoped to Kids, and direct grant scoped to Kids.
    let (status, _, reply) = app
        .send(
            Method::PATCH,
            &format!("/api/v1/users/{target_id}"),
            Some(&mgr_cookie),
            None,
            Some(json!({ "roleAssignments": [{ "roleId": reader_role, "libraryIds": [kids_lib_id] }] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    let (status, _, reply) = app
        .send(
            Method::PUT,
            &format!("/api/v1/users/{target_id}/permissions"),
            Some(&mgr_cookie),
            None,
            Some(json!({ "scopedPermissions": [{ "permission": "library:read", "libraryIds": [default_lib_id] }] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");

    // Owner role cannot be scoped.
    let owner_role_id = db::roles::list(&app.state.db).await.unwrap()[0].role.id;
    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{target_id}"),
            Some(json!({
                "roleAssignments": [{ "roleId": owner_role_id, "libraryIds": [kids_lib_id] }]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");

    // Deleting the empty Kids library must NOT escalate target to all libraries!
    let adult_dir = app._dir.path().join("adult2");
    std::fs::create_dir_all(&adult_dir).unwrap();
    let (status, adult_rf) = app
        .owner(
            Method::POST,
            "/api/v1/root-folders",
            Some(json!({ "path": adult_dir.to_str().unwrap() })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let profile_id = app.seed_profile().await;
    app.seed_movie(
        555,
        "Secret Movie",
        adult_rf["id"].as_i64().unwrap() as i32,
        profile_id,
        None,
    )
    .await;

    let (status, _) = app
        .owner(
            Method::DELETE,
            &format!("/api/v1/libraries/{kids_lib_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Target now has 0 libraries (empty list, never Default's movies).
    let (status, _, movies) = app
        .send(
            Method::GET,
            "/api/v1/movies",
            Some(&target_cookie),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(movies.as_array().unwrap().is_empty(), "{movies}");
}
