//! Radarr v3 compatibility mount (`/radarr/api/v3`): API-key-only auth,
//! case-insensitive path routing, per-route permissions, Radarr-style error
//! bodies and logged 501 fallback (docs/14-compatibility.md, section D).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use api::access::{LibraryEdit, LibraryRead, ProfilesRead, Require};
use api::compat::{
    CompatError, CompatRouteEntry, CompatRouter, RequireApiKey, ValidationFailure,
    ValidationSeverity, dispatch_prefix, verify_entries,
};
use api::{AppState, radarr, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Query};
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use unseen_core::permission::Permission;

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";

#[derive(Debug, Clone)]
struct CapturedEvent {
    level: tracing::Level,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl CaptureLayer {
    fn events(&self) -> Vec<CapturedEvent> {
        self.events.lock().unwrap().clone()
    }

    fn warns(&self) -> Vec<CapturedEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.level == tracing::Level::WARN)
            .collect()
    }
}

struct FieldVisitor(BTreeMap<String, String>);

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor(BTreeMap::new());
        event.record(&mut visitor);
        self.events.lock().unwrap().push(CapturedEvent {
            level: *event.metadata().level(),
            fields: visitor.0,
        });
    }
}

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
    async fn uninitialized() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        let router = router(state.clone());
        Self {
            router,
            state,
            owner: String::new(),
            _dir: dir,
        }
    }

    async fn new() -> Self {
        let mut app = Self::uninitialized().await;
        let setup = json!({
            "name": "Owner",
            "email": "owner@example.com",
            "password": PASSWORD,
        });
        let reply = app
            .send_on(
                &app.router,
                Request::post("/api/v1/setup")
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

    async fn send_on(&self, router: &Router, request: Request<Body>) -> Reply {
        let response = router.clone().oneshot(request).await.unwrap();
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

    async fn send(&self, request: Request<Body>) -> Reply {
        self.send_on(&self.router, request).await
    }

    async fn member(&self, email: &str, permissions: &[&str]) -> (String, String, i32) {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let reply = self
            .send(
                Request::post("/api/auth/sign-up/email")
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
            user_id: Set(user_id.clone()),
            role_id: Set(role.id),
        }
        .insert(&self.state.db)
        .await
        .unwrap();

        (user_id, cookie, role.id)
    }

    async fn create_key(&self, cookie: &str, permissions: &[&str]) -> (String, i64) {
        let body = json!({ "name": "Seerr", "permissions": permissions });
        let reply = self
            .send(
                Request::post("/api/v1/me/api-keys")
                    .header(header::HOST, HOST)
                    .header(header::COOKIE, cookie)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        (
            reply.body["key"].as_str().unwrap().to_owned(),
            reply.body["apiKey"]["id"].as_i64().unwrap(),
        )
    }

    fn mount_compat(&self, compat: CompatRouter) -> Router {
        let base = compat.base().to_owned();
        let service = compat.into_service(&self.state);
        let fallback_service = service.clone();
        Router::new()
            .nest_service(&base, service)
            .fallback(move |req: axum::extract::Request| {
                let fallback_service = fallback_service.clone();
                let base = base.clone();
                async move {
                    match dispatch_prefix(req, &base) {
                        Ok(rewritten) => fallback_service.oneshot(rewritten).await.unwrap(),
                        Err(_) => CompatError::not_found().into_response(),
                    }
                }
            })
    }
}

fn req(method: Method, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, HOST)
}

#[tokio::test]
async fn compat_requires_setup_before_owner_exists() {
    let app = App::uninitialized().await;
    for path in ["/radarr/api/v3/anything", "/Radarr/API/v3/anything"] {
        let reply = app
            .send(req(Method::GET, path).body(Body::empty()).unwrap())
            .await;
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(reply.body["code"], "setup_required", "{path}");
    }
}

#[tokio::test]
async fn request_without_key_returns_401_radarr_json_and_logs_no_warn() {
    let app = App::new().await;

    for uri in [
        "/radarr/api/v3/anything",
        "/radarr/api/v3",
        "/radarr/api/v3/",
        "/Radarr/api/v3/anything",
    ] {
        let capture = CaptureLayer::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let reply = app
            .send(req(Method::GET, uri).body(Body::empty()).unwrap())
            .with_subscriber(subscriber)
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(reply.content_type, "application/json", "{uri}");
        assert_eq!(reply.body, json!({ "error": "Unauthorized" }), "{uri}");
        assert!(
            capture.warns().is_empty(),
            "unauthenticated request must not emit fallback warning: {:?}",
            capture.warns()
        );
    }

    // Empty header and empty query value are rejected with 401.
    let empty_header = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything")
                .header("X-Api-Key", "")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(empty_header.status, StatusCode::UNAUTHORIZED);
    assert_eq!(empty_header.body, json!({ "error": "Unauthorized" }));

    let empty_query = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything?apikey=")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(empty_query.status, StatusCode::UNAUTHORIZED);
    assert_eq!(empty_query.body, json!({ "error": "Unauthorized" }));
}

#[tokio::test]
async fn session_cookie_alone_is_never_accepted_on_compat_router() {
    let app = App::new().await;

    // Live `/radarr/api/v3` fallback refuses a valid Owner session cookie.
    let reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything")
                .header(header::COOKIE, &app.owner)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.content_type, "application/json");
    assert_eq!(reply.body, json!({ "error": "Unauthorized" }));

    // A matched compat route also refuses a session cookie.
    async fn quality_profiles(_: Require<ProfilesRead>) -> Json<Value> {
        Json(json!([]))
    }
    let compat = CompatRouter::with_base(radarr::API_BASE).require(
        Permission::ProfilesRead,
        Method::GET,
        "/qualityprofile",
        quality_profiles,
    );
    let mounted = app.mount_compat(compat);
    let reply = app
        .send_on(
            &mounted,
            req(Method::GET, "/radarr/api/v3/qualityprofile")
                .header(header::COOKIE, &app.owner)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.body, json!({ "error": "Unauthorized" }));
}

#[tokio::test]
async fn invalid_revoked_and_disabled_user_keys_return_401() {
    let app = App::new().await;
    let (user_id, cookie, _) = app.member("m@example.com", &["library:read"]).await;
    let (key, key_id) = app.create_key(&cookie, &["library:read"]).await;

    // Unknown key in header and query.
    let bad_header = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything")
                .header("X-Api-Key", "not-a-real-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(bad_header.status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad_header.body, json!({ "error": "Unauthorized" }));

    let bad_query = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything?apikey=not-a-real-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(bad_query.status, StatusCode::UNAUTHORIZED);

    // Invalid header does not fall back to valid query key or session cookie.
    let bad_header_good_query = app
        .send(
            req(
                Method::GET,
                &format!("/radarr/api/v3/anything?apikey={key}"),
            )
            .header("X-Api-Key", "wrong")
            .header(header::COOKIE, &app.owner)
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(bad_header_good_query.status, StatusCode::UNAUTHORIZED);

    // Disabled user's key fails immediately.
    let disable = app
        .send(
            req(Method::PATCH, &format!("/api/v1/users/{user_id}"))
                .header(header::COOKIE, &app.owner)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "disabled": true }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(disable.status, StatusCode::OK, "{}", disable.body);

    let disabled_reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(disabled_reply.status, StatusCode::UNAUTHORIZED);

    // Re-enable user, then revoke key.
    app.send(
        req(Method::PATCH, &format!("/api/v1/users/{user_id}"))
            .header(header::COOKIE, &app.owner)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "disabled": false }).to_string()))
            .unwrap(),
    )
    .await;

    // Disabling the user also ends its sessions, so revoke the key as the owner.
    let revoke = app
        .send(
            req(Method::DELETE, &format!("/api/v1/api-keys/{key_id}"))
                .header(header::COOKIE, &app.owner)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(revoke.status, StatusCode::NO_CONTENT);

    let revoked_reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(revoked_reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(revoked_reply.body, json!({ "error": "Unauthorized" }));
}

#[tokio::test]
async fn valid_key_via_header_and_query_hits_501_fallback_and_emits_one_warn() {
    let app = App::new().await;
    // Fallback accepts any valid key, even one with zero permissions.
    let (key, _) = app.create_key(&app.owner, &[]).await;

    // 1. Header auth (`X-Api-Key`).
    let capture = CaptureLayer::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let header_reply = app
        .send(
            req(Method::GET, "/radarr/api/v3/anything")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .with_subscriber(subscriber)
        .await;
    assert_eq!(header_reply.status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(header_reply.content_type, "application/json");
    assert_eq!(
        header_reply.body,
        json!({
            "message": "Not implemented",
            "method": "GET",
            "path": "/radarr/api/v3/anything",
        })
    );
    let warns = capture.warns();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert_eq!(
        warns[0].fields.get("method").map(String::as_str),
        Some("GET")
    );
    assert_eq!(
        warns[0].fields.get("path").map(String::as_str),
        Some("/radarr/api/v3/anything")
    );

    // 2. Query auth (`?apikey=`), preserving original path casing and never
    //    leaking the API key into logs or response body.
    let capture = CaptureLayer::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let query_uri = format!("/radarr/api/v3/CustomEndpoint?apikey={key}&term=Tmdb:603");
    let query_reply = app
        .send(req(Method::POST, &query_uri).body(Body::empty()).unwrap())
        .with_subscriber(subscriber)
        .await;
    assert_eq!(query_reply.status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(query_reply.content_type, "application/json");
    assert_eq!(
        query_reply.body,
        json!({
            "message": "Not implemented",
            "method": "POST",
            "path": "/radarr/api/v3/CustomEndpoint",
        })
    );
    assert!(!query_reply.body.to_string().contains(&key));
    let warns = capture.warns();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert_eq!(
        warns[0].fields.get("method").map(String::as_str),
        Some("POST")
    );
    assert_eq!(
        warns[0].fields.get("path").map(String::as_str),
        Some("/radarr/api/v3/CustomEndpoint")
    );
    assert!(
        !warns[0].fields.values().any(|value| value.contains(&key)),
        "API key leaked into warn log: {:?}",
        warns[0].fields
    );

    // 3. Case-insensitive query key (`?apiKey=`) and mixed-case base prefix.
    let capture = CaptureLayer::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let mixed_uri = format!("/Radarr/API/v3/Anything?apiKey={key}");
    let mixed_reply = app
        .send(req(Method::GET, &mixed_uri).body(Body::empty()).unwrap())
        .with_subscriber(subscriber)
        .await;
    assert_eq!(mixed_reply.status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        mixed_reply.body,
        json!({
            "message": "Not implemented",
            "method": "GET",
            "path": "/Radarr/API/v3/Anything",
        })
    );
    let warns = capture.warns();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert_eq!(
        warns[0].fields.get("path").map(String::as_str),
        Some("/Radarr/API/v3/Anything")
    );
}

#[tokio::test]
async fn case_insensitive_routing_and_permission_enforcement() {
    let app = App::new().await;
    let (user_id, cookie, role_id) = app
        .member("seerr@example.com", &["profiles:read", "library:read"])
        .await;
    let (profiles_key, _) = app.create_key(&cookie, &["profiles:read"]).await;
    let (library_key, _) = app.create_key(&cookie, &["library:read"]).await;

    async fn list_profiles(
        caller: Require<ProfilesRead>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        Json(json!({
            "handler": "qualityprofile",
            "userId": caller.user_id(),
            "term": params.get("term"),
        }))
    }

    async fn list_tags(_: Require<LibraryRead>) -> Json<Value> {
        Json(json!([{ "id": 1, "label": "seerr" }]))
    }

    async fn create_tag(_: Require<LibraryEdit>) -> (StatusCode, Json<Value>) {
        (StatusCode::CREATED, Json(json!({ "id": 2 })))
    }

    async fn get_movie(_: Require<LibraryRead>, Path(id): Path<i32>) -> Json<Value> {
        Json(json!({ "id": id }))
    }

    let compat = CompatRouter::with_base(radarr::API_BASE)
        .require(
            Permission::ProfilesRead,
            Method::GET,
            "/qualityprofile",
            list_profiles,
        )
        .require(Permission::LibraryRead, Method::GET, "/tag", list_tags)
        .require(Permission::LibraryEdit, Method::POST, "/tag", create_tag)
        .require(
            Permission::LibraryRead,
            Method::GET,
            "/movie/{id}",
            get_movie,
        );
    assert_eq!(compat.verify(), Ok(()));
    let mounted = app.mount_compat(compat);

    // `/QualityProfile`, `/qualityprofile`, `/QUALITYPROFILE`, trailing slash,
    // and mixed-case mount prefix all reach the exact same handler and preserve
    // query value casing.
    for path in [
        "/radarr/api/v3/QualityProfile?term=Tmdb:603",
        "/radarr/api/v3/qualityprofile?term=Tmdb:603",
        "/radarr/api/v3/QUALITYPROFILE?term=Tmdb:603",
        "/radarr/api/v3/QualityProfile/?term=Tmdb:603",
        "/Radarr/api/v3/QualityProfile?term=Tmdb:603",
    ] {
        let reply = app
            .send_on(
                &mounted,
                req(Method::GET, path)
                    .header("X-Api-Key", &profiles_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {}", reply.body);
        assert_eq!(reply.body["handler"], "qualityprofile", "{path}");
        assert_eq!(reply.body["userId"], user_id, "{path}");
        assert_eq!(reply.body["term"], "Tmdb:603", "{path}");
    }

    // Parameterized route matches case-insensitively and extracts `{id}`.
    let movie = app
        .send_on(
            &mounted,
            req(
                Method::GET,
                &format!("/radarr/api/v3/Movie/42?apikey={library_key}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(movie.status, StatusCode::OK, "{}", movie.body);
    assert_eq!(movie.body["id"], 42);

    // Key without `profiles:read` receives 403 regardless of path casing.
    for path in [
        "/radarr/api/v3/qualityprofile",
        "/radarr/api/v3/QualityProfile",
    ] {
        let forbidden = app
            .send_on(
                &mounted,
                req(Method::GET, path)
                    .header("X-Api-Key", &library_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(forbidden.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(forbidden.content_type, "application/json", "{path}");
        assert_eq!(forbidden.body["message"], "Forbidden");
        assert_eq!(forbidden.body["requiredPermission"], "profiles:read");
    }

    // Same path with different HTTP methods enforces distinct permissions.
    let get_tags = app
        .send_on(
            &mounted,
            req(Method::GET, "/radarr/api/v3/Tag")
                .header("X-Api-Key", &library_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(get_tags.status, StatusCode::OK);

    let post_tag = app
        .send_on(
            &mounted,
            req(Method::POST, "/radarr/api/v3/Tag")
                .header("X-Api-Key", &library_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(post_tag.status, StatusCode::FORBIDDEN);
    assert_eq!(post_tag.body["requiredPermission"], "library:edit");

    // Unimplemented HTTP method on a registered path: 401 without key, 501 +
    // warn log with a valid key.
    let unauth_delete = app
        .send_on(
            &mounted,
            req(Method::DELETE, "/radarr/api/v3/Tag")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(unauth_delete.status, StatusCode::UNAUTHORIZED);

    let capture = CaptureLayer::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let delete_tag = app
        .send_on(
            &mounted,
            req(Method::DELETE, "/radarr/api/v3/Tag")
                .header("X-Api-Key", &library_key)
                .body(Body::empty())
                .unwrap(),
        )
        .with_subscriber(subscriber)
        .await;
    assert_eq!(delete_tag.status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        delete_tag.body,
        json!({
            "message": "Not implemented",
            "method": "DELETE",
            "path": "/radarr/api/v3/Tag",
        })
    );
    let warns = capture.warns();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert_eq!(
        warns[0].fields.get("method").map(String::as_str),
        Some("DELETE")
    );
    assert_eq!(
        warns[0].fields.get("path").map(String::as_str),
        Some("/radarr/api/v3/Tag")
    );

    // Revoking the user's role narrows the API key immediately.
    user_roles::Entity::delete_by_id((user_id, role_id))
        .exec(&app.state.db)
        .await
        .unwrap();
    let narrowed = app
        .send_on(
            &mounted,
            req(Method::GET, "/radarr/api/v3/QualityProfile")
                .header("X-Api-Key", &profiles_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(narrowed.status, StatusCode::FORBIDDEN);
    assert_eq!(narrowed.body["requiredPermission"], "profiles:read");
}

#[tokio::test]
async fn require_api_key_extractor_works_directly_and_rejects_cookies() {
    let app = App::new().await;
    let (_, cookie, _) = app.member("direct@example.com", &["profiles:read"]).await;
    let (read_key, _) = app.create_key(&cookie, &["profiles:read"]).await;
    let (empty_key, _) = app.create_key(&cookie, &[]).await;

    async fn direct(caller: RequireApiKey<ProfilesRead>) -> Json<Value> {
        Json(json!({ "userId": caller.user_id() }))
    }

    let direct_router = Router::new()
        .route("/direct", get(direct))
        .with_state(app.state.clone());

    let with_cookie = app
        .send_on(
            &direct_router,
            req(Method::GET, "/direct")
                .header(header::COOKIE, &app.owner)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(with_cookie.status, StatusCode::UNAUTHORIZED);
    assert_eq!(with_cookie.body, json!({ "error": "Unauthorized" }));

    let lacking_perm = app
        .send_on(
            &direct_router,
            req(Method::GET, "/direct")
                .header("X-Api-Key", &empty_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(lacking_perm.status, StatusCode::FORBIDDEN);
    assert_eq!(lacking_perm.body["requiredPermission"], "profiles:read");

    let allowed = app
        .send_on(
            &direct_router,
            req(Method::GET, "/direct")
                .header("X-Api-Key", &read_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(allowed.status, StatusCode::OK);
}

#[tokio::test]
async fn compat_error_helpers_produce_radarr_json_shapes() {
    let app = App::new().await;
    let (key, _) = app.create_key(&app.owner, &["library:add"]).await;

    async fn single_validation() -> Result<Json<Value>, CompatError> {
        Err(CompatError::validation("tmdbId", "must be positive"))
    }

    async fn detailed_validation() -> Result<Json<Value>, CompatError> {
        Err(CompatError::validation_errors(vec![
            ValidationFailure::new("tmdbId", "A movie with this TMDB ID already exists")
                .attempted_value(603)
                .severity(ValidationSeverity::Error)
                .error_code("MovieExistsValidator"),
            ValidationFailure::new("rootFolderPath", "Path does not exist")
                .attempted_value("/missing")
                .severity(ValidationSeverity::Warning)
                .error_code("RootFolderValidator"),
        ]))
    }

    async fn missing_resource() -> Result<Json<Value>, CompatError> {
        Err(CompatError::not_found())
    }

    async fn malformed() -> Result<Json<Value>, CompatError> {
        Err(CompatError::bad_request("Invalid JSON body"))
    }

    let compat = CompatRouter::with_base(radarr::API_BASE)
        .require(
            Permission::LibraryAdd,
            Method::POST,
            "/validation/single",
            single_validation,
        )
        .require(
            Permission::LibraryAdd,
            Method::POST,
            "/validation/detailed",
            detailed_validation,
        )
        .require(
            Permission::LibraryAdd,
            Method::GET,
            "/missing",
            missing_resource,
        )
        .require(
            Permission::LibraryAdd,
            Method::POST,
            "/malformed",
            malformed,
        );
    let mounted = app.mount_compat(compat);

    let single = app
        .send_on(
            &mounted,
            req(Method::POST, "/radarr/api/v3/validation/single")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(single.status, StatusCode::BAD_REQUEST);
    assert_eq!(single.content_type, "application/json");
    assert_eq!(
        single.body,
        json!([
            {
                "propertyName": "tmdbId",
                "errorMessage": "must be positive",
                "attemptedValue": null,
                "severity": "error",
                "errorCode": null,
            }
        ])
    );

    let detailed = app
        .send_on(
            &mounted,
            req(Method::POST, "/radarr/api/v3/validation/detailed")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(detailed.status, StatusCode::BAD_REQUEST);
    assert_eq!(detailed.content_type, "application/json");
    assert_eq!(
        detailed.body,
        json!([
            {
                "propertyName": "tmdbId",
                "errorMessage": "A movie with this TMDB ID already exists",
                "attemptedValue": 603,
                "severity": "error",
                "errorCode": "MovieExistsValidator",
            },
            {
                "propertyName": "rootFolderPath",
                "errorMessage": "Path does not exist",
                "attemptedValue": "/missing",
                "severity": "warning",
                "errorCode": "RootFolderValidator",
            }
        ])
    );

    let not_found = app
        .send_on(
            &mounted,
            req(Method::GET, "/radarr/api/v3/missing")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(not_found.status, StatusCode::NOT_FOUND);
    assert_eq!(not_found.body, json!({ "message": "NotFound" }));

    let bad_req = app
        .send_on(
            &mounted,
            req(Method::POST, "/radarr/api/v3/malformed")
                .header("X-Api-Key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(bad_req.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_req.body, json!({ "message": "Invalid JSON body" }));
}

#[test]
fn compat_router_verify_catches_missing_and_duplicate_declarations() {
    async fn ok() {}

    assert_eq!(api::radarr_routes().verify(), Ok(()));

    // Exact and case-insensitive duplicate routes fail verification.
    let duplicate = CompatRouter::with_base(radarr::API_BASE)
        .require(Permission::ProfilesRead, Method::GET, "/qualityprofile", ok)
        .require(Permission::ProfilesRead, Method::GET, "/QualityProfile", ok);
    let errors = duplicate.verify().unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.contains("duplicate compat route registration")),
        "{errors:?}"
    );

    // Parameterized routes colliding under different parameter names fail.
    let param_dup = CompatRouter::with_base(radarr::API_BASE)
        .require(Permission::LibraryRead, Method::GET, "/movie/{id}", ok)
        .require(Permission::LibraryRead, Method::GET, "/movie/{movieId}", ok);
    let errors = param_dup.verify().unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.contains("duplicate compat route registration")),
        "{errors:?}"
    );

    // Entry without a permission fails verification.
    let missing = verify_entries(&[CompatRouteEntry {
        method: Method::GET,
        path: "/radarr/api/v3/qualityprofile".to_owned(),
        permission: None,
    }])
    .unwrap_err();
    assert!(
        missing
            .iter()
            .any(|error| error.contains("missing permission declaration")),
        "{missing:?}"
    );
}
