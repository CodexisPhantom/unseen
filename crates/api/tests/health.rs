use api::{AppState, router};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

async fn get(path: &str) -> (StatusCode, String, Value) {
    let (db, _dir) = db::testing::temp_db().await;
    let request = Request::get(path).body(Body::empty()).unwrap();
    let state = AppState::new(db, "test secret", None).await.unwrap();
    let response = router(state).oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, content_type, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn health_returns_version_and_database_status() {
    let (status, content_type, body) = get("/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/json");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["database"], "ok");
}

#[tokio::test]
async fn unknown_route_returns_problem_details() {
    let (status, content_type, body) = get("/api/v1/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(content_type, "application/problem+json");
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["status"], 404);
    assert_eq!(body["type"], "https://unseen.invalid/problems/not_found");
}

#[tokio::test]
async fn compat_prefixes_do_not_fall_back_to_the_app() {
    let (status, content_type, _) = get("/radarr/api/v3/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(content_type, "application/problem+json");
}

#[test]
fn openapi_document_describes_health() {
    let document = serde_json::to_value(api::openapi()).unwrap();
    let ok = &document["paths"]["/api/v1/health"]["get"]["responses"]["200"];
    assert_eq!(
        ok["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Health"
    );
    assert!(document["components"]["schemas"]["Problem"].is_object());
}
