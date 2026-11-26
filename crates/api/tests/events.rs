//! Integration tests for `GET /api/v1/events` (issue #62, docs/05-api.md).

use std::time::Duration;

use api::{AppState, router};
use axum::Router;
use axum::body::{Body, BodyDataStream, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::entities::{quality_profiles, role_permissions, roles, root_folders, user_roles};
use db::movies::{Metadata, NewMovie};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::time::timeout;
use tower::ServiceExt;

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";
const EVENT_TIMEOUT: Duration = Duration::from_secs(2);
const SILENCE_WINDOW: Duration = Duration::from_millis(120);

enum Auth<'a> {
    Cookie(&'a str),
    ApiKey(&'a str),
    None,
}

#[derive(Debug, PartialEq)]
enum SseFrame {
    Comment(String),
    Event { name: String, data: Value },
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

    async fn next_frame(&mut self) -> Option<SseFrame> {
        loop {
            if let Some(frame) = self.take_buffered_frame() {
                return Some(frame);
            }
            let chunk = self.body.next().await?.expect("valid SSE chunk");
            let text = std::str::from_utf8(&chunk).expect("UTF-8 SSE chunk");
            self.buffer.push_str(&text.replace("\r\n", "\n"));
        }
    }

    fn take_buffered_frame(&mut self) -> Option<SseFrame> {
        loop {
            let split = self.buffer.find("\n\n")?;
            let raw = self.buffer[..split].to_owned();
            self.buffer.drain(..split + 2);
            if raw.is_empty() {
                continue;
            }
            let mut event_name = None;
            let mut data_lines = Vec::new();
            let mut comment = None;
            for line in raw.lines() {
                if let Some(rest) = line.strip_prefix(':') {
                    comment = Some(rest.trim().to_owned());
                } else if let Some(rest) = line.strip_prefix("event:") {
                    event_name = Some(rest.trim().to_owned());
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_owned());
                }
            }
            if let Some(name) = event_name {
                let joined = data_lines.join("\n");
                let data = serde_json::from_str(&joined)
                    .unwrap_or_else(|error| panic!("invalid JSON `{joined}`: {error}"));
                return Some(SseFrame::Event { name, data });
            }
            if let Some(comment) = comment {
                return Some(SseFrame::Comment(comment));
            }
        }
    }

    async fn expect_event(&mut self) -> (String, Value) {
        timeout(EVENT_TIMEOUT, async {
            loop {
                match self.next_frame().await.expect("SSE stream ended early") {
                    SseFrame::Event { name, data } => return (name, data),
                    SseFrame::Comment(_) => continue,
                }
            }
        })
        .await
        .expect("timed out waiting for SSE event")
    }

    async fn assert_no_event(&mut self) {
        let received = timeout(SILENCE_WINDOW, async {
            loop {
                match self.next_frame().await {
                    Some(SseFrame::Event { name, data }) => return Some((name, data)),
                    Some(SseFrame::Comment(_)) => continue,
                    None => return None,
                }
            }
        })
        .await;
        if let Ok(Some((name, data))) = received {
            panic!("expected no SSE event, but received `{name}` with {data}");
        }
    }
}

struct App {
    router: Router,
    state: AppState,
    owner_id: String,
    owner_cookie: String,
    _dir: TempDir,
}

impl App {
    async fn new() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        let mut app = Self {
            router: router(state.clone()),
            state,
            owner_id: String::new(),
            owner_cookie: String::new(),
            _dir: dir,
        };
        let body = json!({ "name": "Owner", "email": "owner@example.com", "password": PASSWORD });
        let (status, cookie, reply) = app
            .send(Method::POST, "/api/v1/setup", Auth::None, Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        app.owner_cookie = cookie.unwrap();
        let (status, _, me) = app
            .send(
                Method::GET,
                "/api/v1/me",
                Auth::Cookie(&app.owner_cookie),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{me}");
        app.owner_id = me["user"]["id"].as_str().unwrap().to_owned();
        app
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        auth: Auth<'_>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST);
        match auth {
            Auth::Cookie(cookie) => {
                request = request
                    .header(header::COOKIE, cookie)
                    .header(header::ORIGIN, ORIGIN);
            }
            Auth::ApiKey(key) => {
                request = request.header("X-Api-Key", key);
            }
            Auth::None => {
                request = request.header(header::ORIGIN, ORIGIN);
            }
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
        let (status, _, reply) = self
            .send(method, path, Auth::Cookie(&self.owner_cookie), body)
            .await;
        (status, reply)
    }

    async fn connect(&self, auth: Auth<'_>) -> (StatusCode, HeaderMap, Body) {
        let mut request = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/events")
            .header(header::HOST, HOST);
        match auth {
            Auth::Cookie(cookie) => {
                request = request
                    .header(header::COOKIE, cookie)
                    .header(header::ORIGIN, ORIGIN);
            }
            Auth::ApiKey(key) => {
                request = request.header("X-Api-Key", key);
            }
            Auth::None => {}
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        (status, headers, response.into_body())
    }

    async fn subscribe(&self, auth: Auth<'_>) -> SseStream {
        let (status, _, body) = self.connect(auth).await;
        assert_eq!(status, StatusCode::OK);
        SseStream::new(body)
    }

    async fn create_role(&self, name: &str, permissions: &[&str]) -> i32 {
        let role = roles::ActiveModel {
            name: Set(name.into()),
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
        role.id
    }

    async fn member(&self, email: &str, permissions: &[&str]) -> (String, String, i32) {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": email, "email": email, "password": PASSWORD });
        let (status, cookie, reply) = self
            .send(
                Method::POST,
                "/api/auth/sign-up/email",
                Auth::None,
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let user_id = reply["user"]["id"].as_str().unwrap().to_owned();
        let role_id = self
            .create_role(&format!("Role of {email}"), permissions)
            .await;
        user_roles::ActiveModel {
            user_id: Set(user_id.clone()),
            role_id: Set(role_id),
            all_libraries: Set(true),
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        (user_id, cookie.unwrap(), role_id)
    }
}

#[tokio::test]
async fn unauthenticated_requests_return_401() {
    let app = App::new().await;

    for auth in [
        Auth::None,
        Auth::Cookie("better-auth.session_token=forged.bad"),
        Auth::ApiKey("not-a-valid-key"),
    ] {
        let (status, headers, body) = app.connect(auth).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/problem+json")
        );
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let problem: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(problem["code"], "unauthorized");
    }
}

#[tokio::test]
async fn sse_headers_announce_a_cacheless_event_stream() {
    let app = App::new().await;
    let (status, headers, body) = app.connect(Auth::Cookie(&app.owner_cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.contains("text/event-stream"), "{content_type}");
    assert_eq!(
        headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers
            .get("x-accel-buffering")
            .and_then(|value| value.to_str().ok()),
        Some("no")
    );
    drop(body);
}

#[tokio::test]
async fn user_without_queue_read_receives_no_download_events() {
    let app = App::new().await;
    let (_, with_queue_cookie, _) = app
        .member("queue@example.com", &["queue:read", "library:read"])
        .await;
    let (_, without_queue_cookie, _) = app.member("library@example.com", &["library:read"]).await;
    let (_, no_perms_cookie, _) = app.member("empty@example.com", &[]).await;

    let mut with_queue = app.subscribe(Auth::Cookie(&with_queue_cookie)).await;
    let mut without_queue = app.subscribe(Auth::Cookie(&without_queue_cookie)).await;
    let mut no_perms = app.subscribe(Auth::Cookie(&no_perms_cookie)).await;

    app.state.publish_download_progress(11, 0.75);
    app.state.publish_download_state(11, "importing");
    app.state.publish_movie_changed(99);

    let (name, data) = with_queue.expect_event().await;
    assert_eq!(name, "download.progress");
    assert_eq!(data, json!({ "id": 11, "progress": 0.75 }));
    let (name, data) = with_queue.expect_event().await;
    assert_eq!(name, "download.state");
    assert_eq!(data, json!({ "id": 11, "state": "importing" }));
    let (name, data) = with_queue.expect_event().await;
    assert_eq!(name, "movie.updated");
    assert_eq!(data, json!({ "id": 99 }));

    // The user without `queue:read` skips both `download.*` events and gets
    // `movie.updated` as their first event.
    let (name, data) = without_queue.expect_event().await;
    assert_eq!(name, "movie.updated");
    assert_eq!(data, json!({ "id": 99 }));

    no_perms.assert_no_event().await;
}

#[tokio::test]
async fn removing_queue_read_stops_download_events_without_reconnecting() {
    let app = App::new().await;
    let (user_id, cookie, _) = app.member("live@example.com", &["queue:read"]).await;
    // Give `library:read` directly so we can send a sentinel event after revoking the role.
    let (status, reply) = app
        .owner(
            Method::PUT,
            &format!("/api/v1/users/{user_id}/permissions"),
            Some(json!({ "permissions": ["library:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    let mut stream = app.subscribe(Auth::Cookie(&cookie)).await;

    // While `queue:read` is held, download events arrive.
    app.state.publish_download_progress(1, 0.25);
    let (name, data) = stream.expect_event().await;
    assert_eq!(name, "download.progress");
    assert_eq!(data["id"], 1);

    // Revoke the role holding `queue:read` without closing or reconnecting the SSE stream.
    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{user_id}"),
            Some(json!({ "roleIds": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    let (name, data) = stream.expect_event().await;
    assert_eq!(name, "permissions.changed");
    assert_eq!(data, json!(user_id));

    // Subsequent `download.*` events are blocked immediately on the same connection,
    // while `movie.updated` still arrives.
    app.state.publish_download_progress(2, 0.5);
    app.state.publish_download_state(2, "completed");
    app.state.publish_movie_changed(7);

    let (name, data) = stream.expect_event().await;
    assert_eq!(name, "movie.updated");
    assert_eq!(data, json!({ "id": 7 }));
}

#[tokio::test]
async fn permissions_changed_is_delivered_only_to_affected_users() {
    let app = App::new().await;
    let (target_id, target_cookie, target_role) =
        app.member("target@example.com", &["library:read"]).await;
    let (_, bystander_cookie, _) = app.member("bystander@example.com", &["library:read"]).await;

    let mut owner_stream = app.subscribe(Auth::Cookie(&app.owner_cookie)).await;
    let mut target_stream = app.subscribe(Auth::Cookie(&target_cookie)).await;
    let mut bystander_stream = app.subscribe(Auth::Cookie(&bystander_cookie)).await;

    // 1. Direct grants change on target.
    let (status, reply) = app
        .owner(
            Method::PUT,
            &format!("/api/v1/users/{target_id}/permissions"),
            Some(json!({ "permissions": ["queue:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        target_stream.expect_event().await,
        ("permissions.changed".into(), json!(target_id))
    );

    // 2. Role permission update notifies only holders of that role.
    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/roles/{target_role}"),
            Some(json!({ "permissions": ["library:read", "system:tasks"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        target_stream.expect_event().await,
        ("permissions.changed".into(), json!(target_id))
    );

    // 3. Role deletion notifies former holders of that role.
    let (status, reply) = app
        .owner(
            Method::DELETE,
            &format!("/api/v1/roles/{target_role}?confirm=true"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{reply}");
    assert_eq!(
        target_stream.expect_event().await,
        ("permissions.changed".into(), json!(target_id))
    );

    // 4. Ownership transfer notifies both old and new owners, not bystanders.
    let (status, reply) = app
        .owner(
            Method::POST,
            "/api/v1/owner/transfer",
            Some(json!({
                "targetUserId": target_id,
                "password": PASSWORD,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{reply}");
    assert_eq!(
        owner_stream.expect_event().await,
        ("permissions.changed".into(), json!(app.owner_id))
    );
    assert_eq!(
        target_stream.expect_event().await,
        ("permissions.changed".into(), json!(target_id))
    );

    // The bystander received none of the target's `permissions.changed`
    // events: its next event is this sentinel, published after the transfer.
    app.state.publish_movie_changed(55);
    assert_eq!(
        bystander_stream.expect_event().await,
        ("movie.updated".into(), json!({ "id": 55 }))
    );
    // The new owner has `library:read` again; the old owner has lost every
    // grant, so the sentinel never reaches it.
    assert_eq!(
        target_stream.expect_event().await,
        ("movie.updated".into(), json!({ "id": 55 }))
    );
    owner_stream.assert_no_event().await;
}

#[tokio::test]
async fn movie_updated_and_task_status_are_filtered_by_permission_and_emitted_on_mutations() {
    let app = App::new().await;
    let (_, library_cookie, _) = app.member("lib@example.com", &["library:read"]).await;
    let (_, tasks_cookie, _) = app.member("tasks@example.com", &["system:tasks"]).await;

    let mut library_stream = app.subscribe(Auth::Cookie(&library_cookie)).await;
    let mut tasks_stream = app.subscribe(Auth::Cookie(&tasks_cookie)).await;

    // Seed a movie directly, then mutate and delete it via the HTTP API.
    let root = root_folders::ActiveModel {
        path: Set("/movies".into()),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    let profile = quality_profiles::ActiveModel {
        name: Set("HD".into()),
        upgrade_allowed: Set(true),
        cutoff_quality_id: Set(1),
        items: Set(serde_json::json!([])),
        language: Set("any".into()),
        min_format_score: Set(0),
        cutoff_format_score: Set(0),
        min_upgrade_format_score: Set(1),
        created_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    let movie = db::movies::create(
        &app.state.db,
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
            root_folder_id: root.id,
            folder_name: "The Matrix (1999)".into(),
            monitored: true,
            minimum_availability: "released".into(),
            collection_id: None,
        },
        &[profile.id],
        &[],
    )
    .await
    .unwrap();
    let movie_id = movie.model.id;

    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/movies/{movie_id}"),
            Some(json!({ "monitored": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    app.state.publish_task_status("rss_sync", "running");

    let (status, reply) = app
        .owner(Method::DELETE, &format!("/api/v1/movies/{movie_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{reply}");

    // `library:read` user gets both `movie.updated` events and skips `task.status`.
    assert_eq!(
        library_stream.expect_event().await,
        ("movie.updated".into(), json!({ "id": movie_id }))
    );
    assert_eq!(
        library_stream.expect_event().await,
        ("movie.updated".into(), json!({ "id": movie_id }))
    );
    library_stream.assert_no_event().await;

    // `system:tasks` user skips `movie.updated` and receives `task.status`.
    assert_eq!(
        tasks_stream.expect_event().await,
        (
            "task.status".into(),
            json!({ "name": "rss_sync", "status": "running" })
        )
    );
    tasks_stream.assert_no_event().await;
}

#[tokio::test]
async fn api_key_narrowing_revocation_and_disabling_stop_events_on_open_stream() {
    let app = App::new().await;
    let (user_id, cookie, _) = app
        .member("keyed@example.com", &["queue:read", "library:read"])
        .await;

    // Create an API key scoped only to `queue:read`.
    let (status, _, created) = app
        .send(
            Method::POST,
            "/api/v1/me/api-keys",
            Auth::Cookie(&cookie),
            Some(json!({ "name": "Watcher", "permissions": ["queue:read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let api_key = created["key"].as_str().unwrap().to_owned();
    let key_id = created["apiKey"]["id"].as_i64().unwrap();

    let mut key_stream = app.subscribe(Auth::ApiKey(&api_key)).await;
    let mut session_stream = app.subscribe(Auth::Cookie(&cookie)).await;

    // Key only receives `download.progress`, not `movie.updated`.
    app.state.publish_movie_changed(1);
    app.state.publish_download_progress(5, 0.1);
    assert_eq!(key_stream.expect_event().await.0, "download.progress");
    assert_eq!(session_stream.expect_event().await.0, "movie.updated");
    assert_eq!(session_stream.expect_event().await.0, "download.progress");

    // Revoking the API key stops events on the open key stream immediately.
    let (status, _, _) = app
        .send(
            Method::DELETE,
            &format!("/api/v1/me/api-keys/{key_id}"),
            Auth::Cookie(&cookie),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    app.state.publish_download_progress(6, 0.2);
    assert_eq!(session_stream.expect_event().await.0, "download.progress");
    key_stream.assert_no_event().await;

    // Disabling the user stops events on their open session stream immediately.
    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{user_id}"),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    app.state.publish_movie_changed(2);
    session_stream.assert_no_event().await;
}

#[tokio::test]
async fn changing_a_password_stops_events_on_the_revoked_session() {
    let app = App::new().await;
    let (user_id, cookie, _) = app
        .member("session-revoked@example.com", &["library:read"])
        .await;
    let mut session_stream = app.subscribe(Auth::Cookie(&cookie)).await;

    let (status, reply) = app
        .owner(
            Method::PATCH,
            &format!("/api/v1/users/{user_id}"),
            Some(json!({ "password": "replacement password" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    app.state.publish_movie_changed(1);
    session_stream.assert_no_event().await;
}

#[tokio::test]
async fn an_expired_session_row_stops_events_on_an_open_stream() {
    let app = App::new().await;
    let session = db::auth_schema::session::Entity::find()
        .all(&app.state.db)
        .await
        .unwrap()
        .into_iter()
        .find(|session| session.user_id == app.owner_id)
        .expect("owner session row");
    let id = session.id.clone();
    let mut session_stream = Box::pin(api::events::filtered(
        &app.state.events,
        app.state.db.clone(),
        app.state.policy.clone(),
        app.owner_id.clone(),
        None,
        Some(id.clone()),
    ));

    db::auth_schema::session::ActiveModel {
        id: Set(id.clone()),
        expires_at: Set(chrono::Utc::now() - chrono::Duration::seconds(1)),
        ..Default::default()
    }
    .update(&app.state.db)
    .await
    .unwrap();
    let persisted = db::auth_schema::session::Entity::find_by_id(id.clone())
        .one(&app.state.db)
        .await
        .unwrap()
        .expect("expired session row remains");
    assert!(persisted.expires_at < chrono::Utc::now());

    app.state.publish_movie_changed(1);
    assert!(
        timeout(EVENT_TIMEOUT, session_stream.next())
            .await
            .expect("timed out waiting for SSE stream to close")
            .is_none(),
        "expected the SSE stream to close"
    );

    let still_present = db::auth_schema::session::Entity::find_by_id(id)
        .one(&app.state.db)
        .await
        .unwrap();
    assert!(
        still_present.is_some(),
        "expired session row must still exist"
    );
}
