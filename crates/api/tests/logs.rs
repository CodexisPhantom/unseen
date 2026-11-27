use std::collections::BTreeMap;
use std::path::PathBuf;

use api::logs::{LogEntry, file_layer, log_file_name, prune_expired_on_startup};
use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use chrono::{TimeDelta, Utc};
use db::entities::{role_permissions, roles, user_roles};
use db::secret::Secret;
use db::settings::{Overrides, set_i64};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

const HOST: &str = "unseenarr.test";
const ORIGIN: &str = "http://unseenarr.test";
const PASSWORD: &str = "correct horse";

struct App {
    router: Router,
    state: AppState,
    owner: String,
    logs_dir: PathBuf,
    _dir: TempDir,
}

struct RawReply {
    status: StatusCode,
    content_type: String,
    content_disposition: String,
    bytes: Vec<u8>,
}

impl App {
    async fn new() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let logs_dir = dir.path().join("logs");
        let state = AppState::new(db, "test secret", None)
            .await
            .unwrap()
            .with_logs_dir(logs_dir.clone());
        let mut app = Self {
            router: router(state.clone()),
            state,
            owner: String::new(),
            logs_dir,
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

    async fn send_raw(&self, method: Method, path: &str, cookie: Option<&str>) -> RawReply {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let content_disposition = response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        RawReply {
            status,
            content_type,
            content_disposition,
            bytes,
        }
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

    async fn owner(&self, path: &str) -> (StatusCode, Value) {
        let (status, _, body) = self.send(Method::GET, path, Some(&self.owner), None).await;
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
}

fn entry_line(timestamp: &str, level: &str, target: &str, message: &str, fields: Value) -> String {
    let fields: BTreeMap<String, Value> = serde_json::from_value(fields).unwrap();
    serde_json::to_string(&LogEntry {
        timestamp: timestamp.to_owned(),
        level: level.to_owned(),
        target: target.to_owned(),
        message: message.to_owned(),
        fields,
    })
    .unwrap()
}

#[tokio::test]
async fn lines_logged_before_restart_are_returned_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let logs_dir = dir.path().join("logs");

    // Process run 1: set up owner and write structured log entries to daily files.
    {
        let db = db::connect(dir.path()).await.unwrap();
        let state = AppState::new(db.clone(), "restart-secret", None)
            .await
            .unwrap()
            .with_logs_dir(logs_dir.clone());
        let app_router = router(state);

        let setup_req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/setup")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "name": "Owner", "email": "owner@example.com", "password": PASSWORD })
                    .to_string(),
            ))
            .unwrap();
        let setup_res = app_router.clone().oneshot(setup_req).await.unwrap();
        assert_eq!(setup_res.status(), StatusCode::CREATED);

        let subscriber = tracing_subscriber::registry().with(file_layer(logs_dir.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(phase = "boot", "before restart first");
            tracing::warn!(attempt = 2_u64, "before restart second");
        });

        drop(app_router);
        db.close().await.unwrap();
    }

    // Process run 2: fresh DB connection, fresh AppState and router.
    let db = db::connect(dir.path()).await.unwrap();
    let state = AppState::new(db, "restart-secret", None)
        .await
        .unwrap()
        .with_logs_dir(logs_dir);
    let app_router = router(state);

    let sign_in_req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/sign-in/email")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "email": "owner@example.com", "password": PASSWORD }).to_string(),
        ))
        .unwrap();
    let sign_in_res = app_router.clone().oneshot(sign_in_req).await.unwrap();
    assert_eq!(sign_in_res.status(), StatusCode::OK);
    let cookie = sign_in_res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.contains("session_token="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let logs_req = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/logs")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    let logs_res = app_router.oneshot(logs_req).await.unwrap();
    assert_eq!(logs_res.status(), StatusCode::OK);
    let bytes = to_bytes(logs_res.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();

    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["message"], "before restart second");
    assert_eq!(items[0]["level"], "warn");
    assert_eq!(items[0]["fields"]["attempt"], 2);
    assert_eq!(items[1]["message"], "before restart first");
    assert_eq!(items[1]["level"], "info");
    assert_eq!(items[1]["fields"]["phase"], "boot");
    assert_eq!(body["nextBefore"], Value::Null);
    assert_eq!(body["retentionDays"], 14);
    assert_eq!(body["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn secret_in_logged_struct_never_appears_in_file_or_response() {
    #[derive(Debug)]
    struct IntegrationConfig {
        #[allow(dead_code)]
        name: &'static str,
        #[allow(dead_code)]
        token: Secret,
    }

    let app = App::new().await;
    let holder = IntegrationConfig {
        name: "tmdb",
        token: Secret::new("super-secret-token-xyz"),
    };

    let subscriber = tracing_subscriber::registry().with(file_layer(app.logs_dir.clone()));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(?holder, "loaded integration settings");
    });

    let today_file = log_file_name(Utc::now().date_naive());
    let file_text = std::fs::read_to_string(app.logs_dir.join(&today_file)).unwrap();
    assert!(file_text.contains("loaded integration settings"));
    assert!(file_text.contains("Secret(..)"));
    assert!(
        !file_text.contains("super-secret-token-xyz"),
        "secret leaked to log file: {file_text}"
    );

    let (status, body) = app.owner("/api/v1/logs").await;
    assert_eq!(status, StatusCode::OK);
    let body_text = body.to_string();
    assert!(body_text.contains("Secret(..)"));
    assert!(!body_text.contains("super-secret-token-xyz"));

    let download = app
        .send_raw(
            Method::GET,
            &format!("/api/v1/logs/files/{today_file}"),
            Some(&app.owner),
        )
        .await;
    assert_eq!(download.status, StatusCode::OK);
    let downloaded_text = String::from_utf8(download.bytes).unwrap();
    assert!(!downloaded_text.contains("super-secret-token-xyz"));
}

#[tokio::test]
async fn filters_and_pages_backwards_across_files() {
    let app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();

    let day1 = [
        entry_line(
            "2026-11-24T10:00:00.000Z",
            "trace",
            "api::sync",
            "trace detail",
            json!({}),
        ),
        entry_line(
            "2026-11-24T11:00:00.000Z",
            "info",
            "api::movies",
            "imported Alien",
            json!({ "movieId": 42 }),
        ),
        entry_line(
            "2026-11-24T12:00:00.000Z",
            "error",
            "api::indexer",
            "Indexer Timeout",
            json!({ "host": "tracker.example" }),
        ),
    ]
    .join("\n")
        + "\n";
    let day2 = [
        entry_line(
            "2026-11-25T08:00:00.000Z",
            "debug",
            "api::search",
            "query built",
            json!({ "term": "dune" }),
        ),
        entry_line(
            "2026-11-25T09:00:00.000Z",
            "warn",
            "api::indexer",
            "Rate limit approaching",
            json!({ "remaining": 2 }),
        ),
        entry_line(
            "2026-11-25T10:00:00.000Z",
            "error",
            "api::db",
            "disk busy",
            json!({}),
        ),
    ]
    .join("\n")
        + "\n";

    std::fs::write(app.logs_dir.join("unseenarr-2026-11-24.log"), &day1).unwrap();
    std::fs::write(app.logs_dir.join("unseenarr-2026-11-25.log"), &day2).unwrap();
    std::fs::write(app.logs_dir.join("notes.txt"), "ignored\n").unwrap();

    // Level filter: warn includes warn and error only.
    let (status, warn_body) = app.owner("/api/v1/logs?level=warn").await;
    assert_eq!(status, StatusCode::OK);
    let warn_messages: Vec<&str> = warn_body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        warn_messages,
        ["disk busy", "Rate limit approaching", "Indexer Timeout"]
    );
    assert_eq!(warn_body["files"].as_array().unwrap().len(), 2);
    assert_eq!(warn_body["files"][0]["name"], "unseenarr-2026-11-25.log");
    assert_eq!(warn_body["files"][0]["date"], "2026-11-25");
    assert_eq!(warn_body["files"][1]["name"], "unseenarr-2026-11-24.log");

    // Case-insensitive search matches message, target, and field values.
    let (_, by_msg) = app.owner("/api/v1/logs?search=ALIEN").await;
    assert_eq!(by_msg["items"].as_array().unwrap().len(), 1);
    assert_eq!(by_msg["items"][0]["message"], "imported Alien");

    let (_, by_target) = app.owner("/api/v1/logs?search=API::INDEXER").await;
    assert_eq!(by_target["items"].as_array().unwrap().len(), 2);

    let (_, by_field) = app.owner("/api/v1/logs?search=TRACKER.EXAMPLE").await;
    assert_eq!(by_field["items"].as_array().unwrap().len(), 1);
    assert_eq!(by_field["items"][0]["message"], "Indexer Timeout");

    // Backwards pagination across both files with limit=2.
    let (status, p1) = app.owner("/api/v1/logs?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    let p1_msgs: Vec<&str> = p1["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["message"].as_str().unwrap())
        .collect();
    assert_eq!(p1_msgs, ["disk busy", "Rate limit approaching"]);
    let cursor1 = p1["nextBefore"].as_str().unwrap().to_owned();

    // Appending a newer line does not shift earlier cursors.
    let mut appended = day2.clone();
    appended.push_str(&entry_line(
        "2026-11-25T11:00:00.000Z",
        "info",
        "api::live",
        "brand new line",
        json!({}),
    ));
    appended.push('\n');
    std::fs::write(app.logs_dir.join("unseenarr-2026-11-25.log"), appended).unwrap();

    let (status, p2) = app
        .owner(&format!("/api/v1/logs?limit=2&before={cursor1}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let p2_msgs: Vec<&str> = p2["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["message"].as_str().unwrap())
        .collect();
    assert_eq!(p2_msgs, ["query built", "Indexer Timeout"]);
    let cursor2 = p2["nextBefore"].as_str().unwrap().to_owned();

    let (status, p3) = app
        .owner(&format!("/api/v1/logs?limit=2&before={cursor2}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let p3_msgs: Vec<&str> = p3["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["message"].as_str().unwrap())
        .collect();
    assert_eq!(p3_msgs, ["imported Alien", "trace detail"]);
    assert_eq!(p3["nextBefore"], Value::Null);

    // Invalid query parameters return 422.
    for query in ["?limit=0", "?limit=501", "?level=verbose", "?before=bad"] {
        let (status, _) = app.owner(&format!("/api/v1/logs{query}")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
    }
}

#[tokio::test]
async fn pages_large_log_files_from_the_end_with_byte_offset_cursors() {
    let app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();

    let line_count = 30_000;
    let mut content = String::with_capacity(line_count * 160);
    for index in 0..line_count {
        content.push_str(&entry_line(
            "2026-11-26T10:00:00.000Z",
            "info",
            "api::large_log",
            &format!("entry-{index:05}"),
            json!({ "payload": "large-file paging test" }),
        ));
        content.push('\n');
    }
    std::fs::write(app.logs_dir.join("unseenarr-2026-11-26.log"), &content).unwrap();

    let (status, first_page) = app.owner("/api/v1/logs?limit=3").await;
    assert_eq!(status, StatusCode::OK);
    let messages: Vec<&str> = first_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, ["entry-29999", "entry-29998", "entry-29997"]);

    let cursor = first_page["nextBefore"].as_str().unwrap();
    let (_, cursor_offset) = cursor.split_once(':').unwrap();
    let expected_line = entry_line(
        "2026-11-26T10:00:00.000Z",
        "info",
        "api::large_log",
        "entry-29997",
        json!({ "payload": "large-file paging test" }),
    );
    assert_eq!(
        cursor_offset.parse::<usize>().unwrap(),
        content.rfind(&expected_line).unwrap()
    );

    let (status, second_page) = app
        .owner(&format!("/api/v1/logs?limit=3&before={cursor}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let messages: Vec<&str> = second_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, ["entry-29996", "entry-29995", "entry-29994"]);
}

#[tokio::test]
async fn pages_empty_unterminated_and_split_utf8_log_lines_with_correct_cursors() {
    let app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();

    std::fs::write(app.logs_dir.join("unseenarr-2026-11-24.log"), "").unwrap();
    let unterminated = entry_line(
        "2026-11-25T10:00:00.000Z",
        "info",
        "api::cursor",
        "without trailing newline",
        json!({}),
    );
    std::fs::write(app.logs_dir.join("unseenarr-2026-11-25.log"), &unterminated).unwrap();

    let base = entry_line(
        "2026-11-26T10:00:00.000Z",
        "info",
        "api::utf8",
        "é",
        json!({}),
    );
    let marker = base.find('é').unwrap();
    let padding = "a".repeat(8191 - marker);
    let older = format!("{}{padding}{}", &base[..marker], &base[marker..]);
    assert_eq!(older.as_bytes()[8191], "é".as_bytes()[0]);
    assert_eq!(older.as_bytes()[8192], "é".as_bytes()[1]);
    let newer = entry_line(
        "2026-11-26T11:00:00.000Z",
        "info",
        "api::utf8",
        "newer entry",
        json!({}),
    );
    let split_content = format!("{older}\n{newer}\n");
    std::fs::write(
        app.logs_dir.join("unseenarr-2026-11-26.log"),
        &split_content,
    )
    .unwrap();

    let (status, empty_page) = app.owner("/api/v1/logs?limit=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_page["files"].as_array().unwrap().len(), 3);
    assert_eq!(empty_page["files"][2]["sizeBytes"], 0);
    assert_eq!(empty_page["items"][0]["message"], "newer entry");
    let cursor = empty_page["nextBefore"].as_str().unwrap();
    assert_eq!(cursor, format!("2026-11-26:{}", older.len() + 1));

    let (status, older_page) = app
        .owner(&format!("/api/v1/logs?limit=1&before={cursor}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(older_page["items"].as_array().unwrap().len(), 1);
    assert_eq!(older_page["items"][0]["message"], format!("{padding}é"));
    assert_eq!(older_page["nextBefore"], "2026-11-26:0");

    let (status, unterminated_page) = app
        .owner(&format!(
            "/api/v1/logs?before={}&limit=1",
            older_page["nextBefore"].as_str().unwrap()
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        unterminated_page["items"][0]["message"],
        "without trailing newline"
    );
    assert_eq!(unterminated_page["nextBefore"], Value::Null);
}

#[cfg(unix)]
#[tokio::test]
async fn listing_and_paging_ignore_symlinks_outside_the_logs_directory() {
    use std::os::unix::fs::symlink;

    let app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();
    let outside = app._dir.path().join("outside.log");
    std::fs::write(
        &outside,
        entry_line(
            "2026-11-26T10:00:00.000Z",
            "error",
            "api::secret",
            "outside log secret",
            json!({}),
        ) + "\n",
    )
    .unwrap();
    symlink(&outside, app.logs_dir.join("unseenarr-2026-11-26.log")).unwrap();

    let (status, body) = app.owner("/api/v1/logs").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].as_array().unwrap().is_empty());
    assert!(body["files"].as_array().unwrap().is_empty());

    let reply = app
        .send_raw(
            Method::GET,
            "/api/v1/logs/files/unseenarr-2026-11-26.log",
            Some(&app.owner),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn retention_setting_override_and_startup_pruning() {
    let mut app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();

    // Database setting overrides default 14.
    set_i64(&app.state.db, "logs.retention_days", 7)
        .await
        .unwrap();
    let (status, body) = app.owner("/api/v1/logs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["retentionDays"], 7);

    // Environment override UNSEENARR_SETTING_LOGS_RETENTION_DAYS wins over DB.
    app.state.settings_overrides = Overrides::from_vars([(
        "UNSEENARR_SETTING_LOGS_RETENTION_DAYS".to_owned(),
        "3".to_owned(),
    )]);
    app.router = router(app.state.clone());
    let (status, body) = app.owner("/api/v1/logs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["retentionDays"], 3);

    // Startup pruning uses the effective retention (3 days).
    let today = Utc::now().date_naive();
    let keep_today = log_file_name(today);
    let keep_edge = log_file_name(today - TimeDelta::days(2));
    let drop_expired = log_file_name(today - TimeDelta::days(3));
    let drop_old = log_file_name(today - TimeDelta::days(30));

    for name in [&keep_today, &keep_edge, &drop_expired, &drop_old] {
        std::fs::write(app.logs_dir.join(name), "line\n").unwrap();
    }
    std::fs::write(app.logs_dir.join("keep-me.txt"), "unrelated\n").unwrap();

    prune_expired_on_startup(&app.state).await;

    assert!(app.logs_dir.join(&keep_today).exists());
    assert!(app.logs_dir.join(&keep_edge).exists());
    assert!(!app.logs_dir.join(&drop_expired).exists());
    assert!(!app.logs_dir.join(&drop_old).exists());
    assert!(app.logs_dir.join("keep-me.txt").exists());

    // Invalid override falls back to default 14.
    app.state.settings_overrides = Overrides::from_vars([(
        "UNSEENARR_SETTING_LOGS_RETENTION_DAYS".to_owned(),
        "not-a-number".to_owned(),
    )]);
    app.router = router(app.state.clone());
    let (status, body) = app.owner("/api/v1/logs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["retentionDays"], 14);
}

#[tokio::test]
async fn downloads_log_file_and_blocks_path_traversal() {
    let app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();
    let content = entry_line(
        "2026-11-25T10:00:00.000Z",
        "info",
        "api::test",
        "downloadable line",
        json!({}),
    ) + "\n";
    std::fs::write(app.logs_dir.join("unseenarr-2026-11-25.log"), &content).unwrap();
    // Place a sensitive file in the parent directory to verify traversal cannot reach it.
    std::fs::write(app._dir.path().join("secret"), "top-secret-key").unwrap();

    let reply = app
        .send_raw(
            Method::GET,
            "/api/v1/logs/files/unseenarr-2026-11-25.log",
            Some(&app.owner),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.content_type, "text/plain; charset=utf-8");
    assert_eq!(
        reply.content_disposition,
        "attachment; filename=\"unseenarr-2026-11-25.log\""
    );
    assert_eq!(String::from_utf8(reply.bytes).unwrap(), content);

    // Valid name that does not exist returns 404.
    let missing = app
        .send_raw(
            Method::GET,
            "/api/v1/logs/files/unseenarr-2026-11-01.log",
            Some(&app.owner),
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // Invalid names and path traversal attempts are rejected with 400 or 404.
    for path in [
        "/api/v1/logs/files/..%2Fsecret",
        "/api/v1/logs/files/..%5Csecret",
        "/api/v1/logs/files/secret",
        "/api/v1/logs/files/unseenarr-2026-99-99.log",
        "/api/v1/logs/files/unseenarr-2026-11-25.log.bak",
        "/api/v1/logs/files/..",
    ] {
        let reply = app.send_raw(Method::GET, path, Some(&app.owner)).await;
        assert!(
            matches!(
                reply.status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{path} returned {}",
            reply.status
        );
        assert!(!String::from_utf8_lossy(&reply.bytes).contains("top-secret-key"));
    }
}

#[tokio::test]
async fn requires_auth_and_system_logs_permission() {
    let app = App::new().await;
    std::fs::create_dir_all(&app.logs_dir).unwrap();
    std::fs::write(app.logs_dir.join("unseenarr-2026-11-25.log"), "hello\n").unwrap();

    // 401 without credentials.
    for path in [
        "/api/v1/logs",
        "/api/v1/logs/files/unseenarr-2026-11-25.log",
    ] {
        let (status, _, body) = app.send(Method::GET, path, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(body["code"], "unauthorized");
    }

    // 403 without system:logs.
    let without = app
        .member("no-logs@example.com", &["system:audit", "library:read"])
        .await;
    for path in [
        "/api/v1/logs",
        "/api/v1/logs/files/unseenarr-2026-11-25.log",
    ] {
        let (status, _, body) = app.send(Method::GET, path, Some(&without), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(body["requiredPermission"], "system:logs");
    }

    // 200 with system:logs.
    let with_perm = app.member("has-logs@example.com", &["system:logs"]).await;
    let (status, _, _) = app
        .send(Method::GET, "/api/v1/logs", Some(&with_perm), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let download = app
        .send_raw(
            Method::GET,
            "/api/v1/logs/files/unseenarr-2026-11-25.log",
            Some(&with_perm),
        )
        .await;
    assert_eq!(download.status, StatusCode::OK);
}
