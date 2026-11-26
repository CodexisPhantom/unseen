//! Integration and contract tests for Radarr v3 compatibility endpoints
//! `GET /system/status`, `GET /qualityprofile`, and `GET /rootfolder`
//! (issue #75, docs/14-compatibility.md section D).

use std::collections::BTreeSet;
use std::path::PathBuf;

use api::{AppState, router};
use arr_compat::radarr::RADARR_VERSION;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{movies, role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";

/// `Schema.property` pairs that may be `null` although the upstream schema does
/// not mark them nullable (docs/14).
const NULL_OBJECTS: [&str; 1] = ["QualityProfileQualityItemResource.quality"];

struct Reply {
    status: StatusCode,
    content_type: String,
    cookies: Vec<String>,
    body: Value,
}

impl Reply {
    fn session_pair(&self) -> String {
        self.cookies
            .iter()
            .find(|cookie| cookie.contains("session_token="))
            .expect("a session cookie")
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }
}

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

impl App {
    async fn with_base_url(base_url: Option<&str>) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", base_url).await.unwrap();
        let router = router(state.clone());
        let mut app = Self {
            router,
            state,
            owner: String::new(),
            _dir: dir,
        };
        let setup_path = format!("{}/api/v1/setup", app.state.base_path);
        let setup = json!({
            "name": "Owner",
            "email": "owner@example.com",
            "password": PASSWORD,
        });
        let reply = app
            .send(
                Request::post(&setup_path)
                    .header(header::HOST, HOST)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(setup.to_string()))
                    .unwrap(),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        app.owner = reply.session_pair();
        app
    }

    async fn new() -> Self {
        Self::with_base_url(None).await
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let cookies = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            content_type,
            cookies,
            body,
        }
    }

    async fn member(&self, email: &str, permissions: &[&str]) -> String {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let reply = self
            .send(
                Request::post(format!("{}/api/auth/sign-up/email", self.state.base_path))
                    .header(header::HOST, HOST)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        let user_id = reply.body["user"]["id"].as_str().unwrap().to_owned();
        let cookie = reply.session_pair();

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

        cookie
    }

    async fn create_key(&self, cookie: &str, permissions: &[&str]) -> String {
        let body = json!({ "name": "Seerr", "permissions": permissions });
        let reply = self
            .send(
                Request::post(format!("{}/api/v1/me/api-keys", self.state.base_path))
                    .header(header::HOST, HOST)
                    .header(header::COOKIE, cookie)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        reply.body["key"].as_str().unwrap().to_owned()
    }

    async fn owner_post(&self, path: &str, body: Value) -> Reply {
        self.send(
            Request::post(path)
                .header(header::HOST, HOST)
                .header(header::COOKIE, &self.owner)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

fn req(method: Method, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, HOST)
}

struct SchemaChecker {
    document: Value,
}

impl SchemaChecker {
    fn pinned() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/radarr")
            .join(format!("openapi-{RADARR_VERSION}.json"));
        let text = std::fs::read_to_string(path).expect("pinned Radarr OpenAPI document");
        let document: Value = serde_json::from_str(&text).expect("valid JSON");
        Self { document }
    }

    fn schema(&self, name: &str) -> &Value {
        &self.document["components"]["schemas"][name]
    }

    fn assert_valid(&self, name: &str, value: &Value) {
        let mut problems = Vec::new();
        assert!(
            !self.schema(name).is_null(),
            "pinned schema missing component {name}"
        );
        self.check(self.schema(name), value, name, name, &mut problems);
        assert!(problems.is_empty(), "{name}: {problems:#?}");
    }

    fn check(
        &self,
        schema: &Value,
        value: &Value,
        owner: &str,
        path: &str,
        problems: &mut Vec<String>,
    ) {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference.rsplit('/').next().expect("ref name");
            return self.check(self.schema(name), value, name, path, problems);
        }
        if value.is_null() {
            if schema["nullable"] != true {
                problems.push(format!("{path}: null where schema is not nullable"));
            }
            return;
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                problems.push(format!("{path}: {value} is not one of {allowed:?}"));
            }
            return;
        }
        let kind = schema.get("type").and_then(Value::as_str);
        let format = schema.get("format").and_then(Value::as_str);
        let fits = match kind {
            None => true,
            Some("string") => value.is_string(),
            Some("boolean") => value.is_boolean(),
            Some("number") => value.is_number(),
            Some("integer") => match format {
                Some("int32") => value
                    .as_i64()
                    .is_some_and(|number| i32::try_from(number).is_ok()),
                _ => value.is_i64() || value.is_u64(),
            },
            Some("array") => value.is_array(),
            Some("object") => value.is_object(),
            Some(other) => panic!("{path}: unexpected schema type {other}"),
        };
        if !fits {
            problems.push(format!(
                "{path}: {value} does not match {kind:?} {format:?}"
            ));
            return;
        }
        match (kind, format, value) {
            (Some("string"), Some("date-time"), Value::String(text))
                if chrono::DateTime::parse_from_rfc3339(text).is_err() =>
            {
                problems.push(format!("{path}: {text:?} is not RFC 3339"));
            }
            (Some("array"), _, Value::Array(items)) => {
                for (index, item) in items.iter().enumerate() {
                    self.check(
                        &schema["items"],
                        item,
                        owner,
                        &format!("{path}[{index}]"),
                        problems,
                    );
                }
            }
            (Some("object"), _, Value::Object(map)) => {
                let properties = schema["properties"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                for (key, sub) in &properties {
                    match map.get(key) {
                        None => problems.push(format!("{path}.{key}: missing")),
                        Some(Value::Null)
                            if NULL_OBJECTS.contains(&format!("{owner}.{key}").as_str()) => {}
                        Some(inner) => {
                            self.check(sub, inner, owner, &format!("{path}.{key}"), problems)
                        }
                    }
                }
                if schema["additionalProperties"] == false {
                    for key in map.keys().filter(|key| !properties.contains_key(*key)) {
                        problems.push(format!("{path}.{key}: not in schema"));
                    }
                }
            }
            _ => {}
        }
    }
}

fn object_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("JSON object")
        .keys()
        .cloned()
        .collect()
}

#[tokio::test]
async fn system_status_matches_schema_recorded_keys_and_permissions() {
    let app = App::new().await;
    let settings_cookie = app.member("settings@example.com", &["settings:read"]).await;
    let settings_key = app.create_key(&settings_cookie, &["settings:read"]).await;
    let profiles_cookie = app.member("profiles@example.com", &["profiles:read"]).await;
    let profiles_key = app.create_key(&profiles_cookie, &["profiles:read"]).await;

    // Missing `settings:read` -> 403.
    let forbidden = app
        .send(
            req(Method::GET, "/radarr/api/v3/system/status")
                .header("X-Api-Key", &profiles_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.body["requiredPermission"], "settings:read");

    // Case-insensitive path + `X-Api-Key` and `?apikey=` both succeed.
    let reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/System/Status")
                .header("X-Api-Key", &settings_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.content_type, "application/json");
    assert_eq!(reply.body["appName"], "Radarr");
    assert_eq!(reply.body["version"], RADARR_VERSION);
    assert_eq!(reply.body["urlBase"], "/radarr");
    assert_eq!(reply.body["startTime"], app.state.started_at);

    let via_query = app
        .send(
            req(
                Method::GET,
                &format!("/radarr/api/v3/system/status?apikey={settings_key}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(via_query.status, StatusCode::OK);
    assert_eq!(via_query.body, reply.body);

    // Validates against pinned OpenAPI schema and matches recorded key set.
    let checker = SchemaChecker::pinned();
    checker.assert_valid("SystemResource", &reply.body);
    let recorded: Value =
        serde_json::from_str(include_str!("../../../fixtures/radarr/system_status.json")).unwrap();
    assert_eq!(object_keys(&reply.body), object_keys(&recorded));
}

#[tokio::test]
async fn system_status_includes_subpath_in_url_base() {
    let app = App::with_base_url(Some("http://unseen.test/unseen")).await;
    let key = app.create_key(&app.owner, &["settings:read"]).await;

    let reply = app
        .send(
            req(Method::GET, "/unseen/radarr/api/v3/system/status")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["urlBase"], "/unseen/radarr");
    assert_eq!(reply.body["appName"], "Radarr");
    assert_eq!(reply.body["version"], RADARR_VERSION);
}

#[tokio::test]
async fn qualityprofile_lists_unseen_profiles_matches_schema_and_enforces_permission() {
    let app = App::new().await;
    let profiles_cookie = app.member("profiles@example.com", &["profiles:read"]).await;
    let profiles_key = app.create_key(&profiles_cookie, &["profiles:read"]).await;
    let settings_cookie = app.member("settings@example.com", &["settings:read"]).await;
    let settings_key = app.create_key(&settings_cookie, &["settings:read"]).await;

    // Create a custom profile with a quality group, language, and format scores.
    let created = app
        .owner_post(
            "/api/v1/quality-profiles",
            json!({
                "name": "Seerr 1080p",
                "upgradeAllowed": true,
                "cutoff": 1000,
                "items": [
                    { "type": "quality", "qualityId": 5, "allowed": false },
                    {
                        "type": "group",
                        "id": 1000,
                        "name": "WEB 1080p",
                        "qualityIds": [15, 3],
                        "allowed": true
                    },
                    { "type": "quality", "qualityId": 7, "allowed": true }
                ],
                "language": "en",
                "minFormatScore": 0,
                "cutoffFormatScore": 100,
                "minUpgradeFormatScore": 1
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let created_id = created.body["id"].as_i64().unwrap();

    // Missing `profiles:read` -> 403.
    let forbidden = app
        .send(
            req(Method::GET, "/radarr/api/v3/qualityprofile")
                .header("X-Api-Key", &settings_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.body["requiredPermission"], "profiles:read");

    // Writes are out of scope -> 501.
    let write_reply = app
        .send(
            req(Method::POST, "/radarr/api/v3/qualityprofile")
                .header("X-Api-Key", &profiles_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(write_reply.status, StatusCode::NOT_IMPLEMENTED);

    // Seerr requests `/qualityProfile` (camelCase); compat router matches case-insensitively.
    let reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/qualityProfile")
                .header("X-Api-Key", &profiles_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.content_type, "application/json");
    let list = reply.body.as_array().expect("JSON array");
    assert!(!list.is_empty());

    let custom = list
        .iter()
        .find(|item| item["id"].as_i64() == Some(created_id))
        .expect("custom profile present");
    assert_eq!(custom["name"], "Seerr 1080p");
    assert_eq!(custom["upgradeAllowed"], true);
    assert_eq!(custom["cutoff"], 1000);
    assert_eq!(custom["language"], json!({ "id": 1, "name": "English" }));
    assert_eq!(custom["minFormatScore"], 0);
    assert_eq!(custom["cutoffFormatScore"], 100);
    assert_eq!(custom["minUpgradeFormatScore"], 1);
    assert_eq!(custom["formatItems"], json!([]));

    // Verify group mapping: `quality: null`, group `id` and `name`, member `items`.
    let group = &custom["items"][1];
    assert_eq!(group["id"], 1000);
    assert_eq!(group["name"], "WEB 1080p");
    assert_eq!(group["quality"], Value::Null);
    assert_eq!(group["allowed"], true);
    assert_eq!(group["items"].as_array().unwrap().len(), 2);

    // Validate every profile against pinned OpenAPI schema and recorded key set.
    let checker = SchemaChecker::pinned();
    let recorded: Vec<Value> =
        serde_json::from_str(include_str!("../../../fixtures/radarr/qualityprofile.json")).unwrap();
    let expected_keys = object_keys(&recorded[0]);
    for profile in list {
        checker.assert_valid("QualityProfileResource", profile);
        assert_eq!(object_keys(profile), expected_keys);
    }
}

#[tokio::test]
async fn rootfolder_reports_free_space_unmapped_folders_and_inaccessible_state() {
    let app = App::new().await;
    let settings_cookie = app.member("settings@example.com", &["settings:read"]).await;
    let settings_key = app.create_key(&settings_cookie, &["settings:read"]).await;
    let profiles_cookie = app.member("profiles@example.com", &["profiles:read"]).await;
    let profiles_key = app.create_key(&profiles_cookie, &["profiles:read"]).await;

    // Missing `settings:read` -> 403.
    let forbidden = app
        .send(
            req(Method::GET, "/radarr/api/v3/rootfolder")
                .header("X-Api-Key", &profiles_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.body["requiredPermission"], "settings:read");

    // Writes are out of scope -> 501.
    let write_reply = app
        .send(
            req(Method::POST, "/radarr/api/v3/rootfolder")
                .header("X-Api-Key", &settings_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(write_reply.status, StatusCode::NOT_IMPLEMENTED);

    // Create two root folders: one active with mapped + unmapped subdirs, one removed after registration.
    let active_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(active_dir.path().join("The Matrix (1999)")).unwrap();
    std::fs::create_dir(active_dir.path().join("Unmatched Movie (2020)")).unwrap();
    std::fs::write(active_dir.path().join("notes.txt"), b"x").unwrap();

    let vanished_dir = tempfile::tempdir().unwrap();

    let active_reply = app
        .owner_post(
            "/api/v1/root-folders",
            json!({ "path": active_dir.path().to_str().unwrap() }),
        )
        .await;
    assert_eq!(
        active_reply.status,
        StatusCode::CREATED,
        "{}",
        active_reply.body
    );
    let active_id = active_reply.body["id"].as_i64().unwrap() as i32;

    let vanished_reply = app
        .owner_post(
            "/api/v1/root-folders",
            json!({ "path": vanished_dir.path().to_str().unwrap() }),
        )
        .await;
    assert_eq!(
        vanished_reply.status,
        StatusCode::CREATED,
        "{}",
        vanished_reply.body
    );
    let vanished_id = vanished_reply.body["id"].as_i64().unwrap() as i32;

    // Record one movie so only `Unmatched Movie (2020)` is unmapped.
    movies::ActiveModel {
        tmdb_id: Set(603),
        title: Set("The Matrix".into()),
        sort_title: Set("matrix".into()),
        year: Set(1999),
        status: Set("released".into()),
        root_folder_id: Set(active_id),
        folder_name: Set("The Matrix (1999)".into()),
        monitored: Set(true),
        minimum_availability: Set("released".into()),
        added_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    // Remove the second folder from disk so it reports inaccessible.
    drop(vanished_dir);

    let reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/rootfolder")
                .header("X-Api-Key", &settings_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.content_type, "application/json");
    let list = reply.body.as_array().expect("JSON array");
    assert_eq!(list.len(), 2);

    let active = list
        .iter()
        .find(|item| item["id"].as_i64() == Some(i64::from(active_id)))
        .expect("active root folder");
    assert_eq!(active["accessible"], true);
    assert!(active["freeSpace"].as_i64().unwrap() > 0);
    let unmapped = active["unmappedFolders"].as_array().unwrap();
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0]["name"], "Unmatched Movie (2020)");
    assert_eq!(unmapped[0]["relativePath"], "Unmatched Movie (2020)");
    assert!(
        unmapped[0]["path"]
            .as_str()
            .unwrap()
            .ends_with("Unmatched Movie (2020)")
    );

    let vanished = list
        .iter()
        .find(|item| item["id"].as_i64() == Some(i64::from(vanished_id)))
        .expect("vanished root folder");
    assert_eq!(vanished["accessible"], false);
    assert_eq!(vanished["freeSpace"], Value::Null);
    assert_eq!(vanished["unmappedFolders"], json!([]));

    // Validate every root folder against pinned OpenAPI schema and recorded key sets.
    let checker = SchemaChecker::pinned();
    let recorded: Vec<Value> =
        serde_json::from_str(include_str!("../../../fixtures/radarr/rootfolder.json")).unwrap();
    let expected_keys = object_keys(&recorded[0]);
    let expected_unmapped_keys = object_keys(&recorded[0]["unmappedFolders"][0]);
    for folder in list {
        checker.assert_valid("RootFolderResource", folder);
        assert_eq!(object_keys(folder), expected_keys);
        for item in folder["unmappedFolders"].as_array().unwrap() {
            assert_eq!(object_keys(item), expected_unmapped_keys);
        }
    }
}

#[tokio::test]
async fn seerr_test_connection_sequence_succeeds_with_status_profiles_and_root_folders() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let created_folder = app
        .owner_post(
            "/api/v1/root-folders",
            json!({ "path": dir.path().to_str().unwrap() }),
        )
        .await;
    assert_eq!(created_folder.status, StatusCode::CREATED);

    let seerr_cookie = app
        .member("seerr@example.com", &["settings:read", "profiles:read"])
        .await;
    let seerr_key = app
        .create_key(&seerr_cookie, &["settings:read", "profiles:read"])
        .await;

    // Exact sequence Seerr performs when clicking "Test" on a Radarr server entry.
    let status = app
        .send(
            req(Method::GET, "/radarr/api/v3/system/status")
                .header("X-Api-Key", &seerr_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status.status, StatusCode::OK, "{}", status.body);
    assert_eq!(status.body["appName"], "Radarr");
    assert_eq!(status.body["version"], RADARR_VERSION);
    assert_eq!(status.body["urlBase"], "/radarr");

    let profiles = app
        .send(
            req(Method::GET, "/radarr/api/v3/qualityProfile")
                .header("X-Api-Key", &seerr_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(profiles.status, StatusCode::OK, "{}", profiles.body);
    assert!(!profiles.body.as_array().unwrap().is_empty());

    let root_folders = app
        .send(
            req(Method::GET, "/radarr/api/v3/rootfolder")
                .header("X-Api-Key", &seerr_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(root_folders.status, StatusCode::OK, "{}", root_folders.body);
    assert_eq!(root_folders.body.as_array().unwrap().len(), 1);
}
