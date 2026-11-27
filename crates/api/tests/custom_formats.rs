use std::path::Path;

use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use db::entities::{quality_profile_custom_formats, role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseenarr.test";
const ORIGIN: &str = "http://unseenarr.test";
const PASSWORD: &str = "correct horse";
const PATH: &str = "/api/v1/custom-formats";

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

    async fn send_raw(
        &self,
        method: Method,
        path: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Vec<u8>) {
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
        (status, cookie, bytes.to_vec())
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let (status, cookie, bytes) = self.send_raw(method, path, cookie, body).await;
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

fn sample_body(name: &str) -> Value {
    json!({
        "name": name,
        "includeCustomFormatWhenRenaming": false,
        "specifications": [
            {
                "name": "x265",
                "implementation": "ReleaseTitleSpecification",
                "negate": false,
                "required": true,
                "fields": [
                    { "name": "value", "value": "x265|hevc" }
                ]
            }
        ]
    })
}

fn strip_ids(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                strip_ids(item);
            }
        }
        Value::Object(map) => {
            map.remove("id");
            for val in map.values_mut() {
                strip_ids(val);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn create_list_get_update_delete_and_audit() {
    let app = App::new().await;

    let (status, created) = app
        .owner(Method::POST, PATH, Some(sample_body("x265")))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["name"], "x265");
    assert_eq!(created["includeCustomFormatWhenRenaming"], false);
    assert_eq!(created["specifications"].as_array().unwrap().len(), 1);

    // Duplicate name is rejected with 409.
    let (status, conflict) = app
        .owner(Method::POST, PATH, Some(sample_body("x265")))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "name_taken");

    let (status, list) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    let item_path = format!("{PATH}/{id}");
    let (status, fetched) = app.owner(Method::GET, &item_path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["id"], id);

    let (status, updated) = app
        .owner(
            Method::PATCH,
            &item_path,
            Some(json!({ "name": "HEVC", "includeCustomFormatWhenRenaming": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "HEVC");
    assert_eq!(updated["includeCustomFormatWhenRenaming"], true);

    let (status, _) = app.owner(Method::DELETE, &item_path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = app.owner(Method::GET, &item_path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Audit log recorded create, update, and delete.
    let (status, audit) = app
        .owner(
            Method::GET,
            "/api/v1/audit-log?targetKind=custom_format",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{audit}");
    let actions: Vec<&str> = audit["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        vec![
            "custom_format.deleted",
            "custom_format.updated",
            "custom_format.created",
        ]
    );
}

#[tokio::test]
async fn radarr_export_imports_and_exports_byte_equivalent_json() {
    let app = App::new().await;
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/radarr/customformat.json");
    let raw = std::fs::read_to_string(&fixture_path).expect("radarr customformat fixture");
    let mut expected: Value = serde_json::from_str(&raw).expect("valid json");

    let (status, imported) = app
        .owner(
            Method::POST,
            &format!("{PATH}/import"),
            Some(expected.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let imported_items = imported.as_array().unwrap();
    assert_eq!(imported_items.len(), 2);

    // Export all formats.
    let (status, _, export_bytes) = app
        .send_raw(
            Method::GET,
            &format!("{PATH}/export"),
            Some(&app.owner),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let mut exported: Value = serde_json::from_slice(&export_bytes).unwrap();

    strip_ids(&mut expected);
    // Sort both arrays by name so ordering matches DB name order.
    expected
        .as_array_mut()
        .unwrap()
        .sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    exported
        .as_array_mut()
        .unwrap()
        .sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    assert_eq!(exported, expected);

    // Single-format export and re-import round-trip produces byte-identical JSON.
    let first_id = imported_items[0]["id"].as_i64().unwrap();
    let (status, _, single_bytes_1) = app
        .send_raw(
            Method::GET,
            &format!("{PATH}/{first_id}/export"),
            Some(&app.owner),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Delete all, re-import the exported single JSON, export again -> exact byte equality.
    for item in imported_items {
        let id = item["id"].as_i64().unwrap();
        let (status, _) = app
            .owner(Method::DELETE, &format!("{PATH}/{id}"), None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let single_val: Value = serde_json::from_slice(&single_bytes_1).unwrap();
    let (status, reimported) = app
        .owner(Method::POST, &format!("{PATH}/import"), Some(single_val))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{reimported}");
    let new_id = reimported[0]["id"].as_i64().unwrap();
    let (status, _, single_bytes_2) = app
        .send_raw(
            Method::GET,
            &format!("{PATH}/{new_id}/export"),
            Some(&app.owner),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(single_bytes_1, single_bytes_2);
}

#[tokio::test]
async fn deleting_a_format_removes_its_scores_from_every_profile() {
    let app = App::new().await;
    let (status, created) = app
        .owner(Method::POST, PATH, Some(sample_body("Remux")))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let format_id = created["id"].as_i64().unwrap() as i32;

    let profiles = db::profiles::list(&app.state.db).await.unwrap();
    assert!(profiles.len() >= 2);
    for profile in &profiles {
        quality_profile_custom_formats::ActiveModel {
            quality_profile_id: Set(profile.id),
            custom_format_id: Set(format_id),
            score: Set(250),
        }
        .insert(&app.state.db)
        .await
        .unwrap();
    }

    let before = quality_profile_custom_formats::Entity::find()
        .filter(quality_profile_custom_formats::Column::CustomFormatId.eq(format_id))
        .all(&app.state.db)
        .await
        .unwrap();
    assert_eq!(before.len(), profiles.len());

    let (status, _) = app
        .owner(Method::DELETE, &format!("{PATH}/{format_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let after = quality_profile_custom_formats::Entity::find()
        .filter(quality_profile_custom_formats::Column::CustomFormatId.eq(format_id))
        .all(&app.state.db)
        .await
        .unwrap();
    assert!(after.is_empty());
}

#[tokio::test]
async fn invalid_regex_returns_validation_error_with_position() {
    let app = App::new().await;
    let body = json!({
        "name": "Bad Regex",
        "includeCustomFormatWhenRenaming": false,
        "specifications": [
            {
                "name": "Broken",
                "implementation": "ReleaseTitleSpecification",
                "negate": false,
                "required": true,
                "fields": [
                    { "name": "value", "value": "foo)bar" }
                ]
            }
        ]
    });

    let (status, reply) = app.owner(Method::POST, PATH, Some(body.clone())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["code"], "validation");
    assert_eq!(
        reply["errors"][0]["field"],
        "specifications[0].fields.value"
    );
    let message = reply["errors"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("position 3"),
        "expected position in message: {message}"
    );

    // Import also rejects invalid regex with position.
    let (status, reply) = app
        .owner(Method::POST, &format!("{PATH}/import"), Some(body))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert!(
        reply["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("position 3")
    );
}

#[tokio::test]
async fn test_endpoint_returns_matching_formats_and_profile_scores() {
    let app = App::new().await;
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/radarr/customformat.json");
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture_path).unwrap()).unwrap();
    let (status, imported) = app
        .owner(Method::POST, &format!("{PATH}/import"), Some(raw))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");

    let x265_hd_id = imported
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "x265 (HD)")
        .unwrap()["id"]
        .as_i64()
        .unwrap() as i32;

    let profiles = db::profiles::list(&app.state.db).await.unwrap();
    let hd_profile = profiles.iter().find(|p| p.name == "HD-1080p").unwrap();
    quality_profile_custom_formats::ActiveModel {
        quality_profile_id: Set(hd_profile.id),
        custom_format_id: Set(x265_hd_id),
        score: Set(-10000),
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let (status, result) = app
        .owner(
            Method::POST,
            &format!("{PATH}/test"),
            Some(json!({
                "releaseName": "The.Matrix.1999.1080p.BluRay.x265-GROUP"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");

    let matched: Vec<&str> = result["matchedFormats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(matched, vec!["x265 (HD)"]);

    let hd_score = result["profileScores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["profileName"] == "HD-1080p")
        .unwrap();
    assert_eq!(hd_score["totalScore"], -10000);
    assert_eq!(hd_score["formatScores"][0]["score"], -10000);

    let sd_score = result["profileScores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["profileName"] == "SD")
        .unwrap();
    assert_eq!(sd_score["totalScore"], 0);
}

#[tokio::test]
async fn permissions_separate_read_and_manage() {
    let app = App::new().await;
    let reader = app.member("cf-read@example.com", &["profiles:read"]).await;
    let nobody = app.member("cf-none@example.com", &[]).await;

    // Reader can list, export, test.
    let (status, _, _) = app.send(Method::GET, PATH, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app
        .send(Method::GET, &format!("{PATH}/export"), Some(&reader), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app
        .send(
            Method::POST,
            &format!("{PATH}/test"),
            Some(&reader),
            Some(json!({ "releaseName": "Movie.2020.1080p.BluRay.x264-GROUP" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Reader cannot create, import, or delete.
    let (status, _, _) = app
        .send(Method::POST, PATH, Some(&reader), Some(sample_body("X")))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = app
        .send(
            Method::POST,
            &format!("{PATH}/import"),
            Some(&reader),
            Some(sample_body("X")),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Nobody cannot read or test.
    let (status, _, _) = app.send(Method::GET, PATH, Some(&nobody), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = app
        .send(
            Method::POST,
            &format!("{PATH}/test"),
            Some(&nobody),
            Some(json!({ "releaseName": "Movie.2020.1080p.BluRay.x264-GROUP" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
