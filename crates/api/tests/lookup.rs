use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
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

const SEARCH: &str = include_str!("../../../fixtures/tmdb/search_movie.json");
const MOVIE: &str = include_str!("../../../fixtures/tmdb/movie_603.json");
const FIND: &str = include_str!("../../../fixtures/tmdb/find_imdb.json");
const FIND_EMPTY: &str = include_str!("../../../fixtures/tmdb/find_empty.json");
const INVALID_KEY: &str = include_str!("../../../fixtures/tmdb/error_invalid_key.json");
const NOT_FOUND: &str = include_str!("../../../fixtures/tmdb/error_not_found.json");

const LOOKUP: &str = "/api/v1/movies/lookup";
const SETTINGS: &str = "/api/v1/settings/tmdb";
const KEY: &str = "fixture-key-0123456789abcdef";

fn json_body(body: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

/// An app whose TMDB key is set, with a mock server that answers `path`.
async fn configured(
    server: &MockServer,
    path_: &str,
    response: ResponseTemplate,
    expected_requests: u64,
) -> App {
    Mock::given(method("GET"))
        .and(path(path_))
        .and(query_param("api_key", KEY))
        .respond_with(response)
        .expect(expected_requests)
        .mount(server)
        .await;
    let app = App::new(server).await;
    db::settings::set_secret(
        &app.state.db,
        &app.state.secrets,
        "tmdb.api_key",
        &db::secret::Secret::new(KEY),
    )
    .await
    .unwrap();
    app
}

async fn lookup(app: &App, term: &str) -> (StatusCode, Value) {
    let uri = format!(
        "{LOOKUP}?term={}",
        term.replace(':', "%3A").replace(' ', "%20")
    );
    app.owner(Method::GET, &uri, None).await
}

#[tokio::test]
async fn lookup_by_text_returns_the_fixture_results() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/search/movie", json_body(SEARCH, 200), 1).await;

    let (status, reply) = lookup(&app, "the matrix").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let results = reply.as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["tmdbId"], 603);
    assert_eq!(results[0]["title"], "The Matrix");
    assert_eq!(results[0]["year"], 1999);
    assert_eq!(results[0]["releaseDate"], "1999-03-30");
    assert_eq!(results[0]["posterPath"], "/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg");
    assert_eq!(results[0]["imdbId"], Value::Null);
    assert_eq!(results[2]["year"], Value::Null);

    // The request carried the term and the default language.
    let requests = server.received_requests().await.unwrap();
    let query = requests[0].url.query().unwrap();
    assert!(query.contains("query=the+matrix") || query.contains("query=the%20matrix"));
    assert!(query.contains("language=en-US"));
}

#[tokio::test]
async fn lookup_by_tmdb_id_returns_the_fixture_movie_with_its_imdb_id() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/movie/603", json_body(MOVIE, 200), 1).await;

    let (status, reply) = lookup(&app, "tmdb:603").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply.as_array().unwrap().len(), 1);
    assert_eq!(reply[0]["tmdbId"], 603);
    assert_eq!(reply[0]["imdbId"], "tt0133093");
    assert_eq!(reply[0]["title"], "The Matrix");
}

#[tokio::test]
async fn lookup_by_imdb_id_returns_the_fixture_movie() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/find/tt0133093", json_body(FIND, 200), 1).await;

    let (status, reply) = lookup(&app, "imdb:tt0133093").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply[0]["tmdbId"], 603);
    assert_eq!(reply[0]["imdbId"], "tt0133093");
}

#[tokio::test]
async fn an_id_that_matches_nothing_gives_an_empty_list() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/movie/1", json_body(NOT_FOUND, 404), 1).await;
    Mock::given(method("GET"))
        .and(path("/3/find/tt0000001"))
        .respond_with(json_body(FIND_EMPTY, 200))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(lookup(&app, "tmdb:1").await, (StatusCode::OK, json!([])));
    assert_eq!(
        lookup(&app, "imdb:tt0000001").await,
        (StatusCode::OK, json!([]))
    );
}

#[tokio::test]
async fn a_missing_key_gives_tmdb_not_configured_without_calling_tmdb() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let app = App::new(&server).await;

    for term in ["matrix", "tmdb:603", "imdb:tt0133093"] {
        let (status, reply) = lookup(&app, term).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "tmdb_not_configured");
        assert_eq!(
            reply["detail"],
            "Set the TMDB API key in settings before searching."
        );
    }
}

#[tokio::test]
async fn a_key_that_cannot_be_decrypted_asks_for_it_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let app = App::new(&server).await;
    db::settings::set_secret(
        &app.state.db,
        &db::secret::SecretKey::derive("a lost secret"),
        "tmdb.api_key",
        &db::secret::Secret::new(KEY),
    )
    .await
    .unwrap();

    let (status, reply) = lookup(&app, "matrix").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reply["code"], "tmdb_not_configured");
    assert!(reply["detail"].as_str().unwrap().contains("Enter it again"));

    let (_, settings) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(settings["hasApiKey"], false);
    assert_eq!(settings["apiKeyMustBeReentered"], true);
}

#[tokio::test]
async fn a_bad_term_is_a_validation_error() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/search/movie", json_body(SEARCH, 200), 0).await;

    for uri in [
        LOOKUP.to_owned(),
        format!("{LOOKUP}?term="),
        format!("{LOOKUP}?term=%20%20"),
        format!("{LOOKUP}?term=tmdb%3Aabc"),
        format!("{LOOKUP}?term=imdb%3A123"),
    ] {
        let (status, reply) = app.owner(Method::GET, &uri, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}: {reply}");
        assert_eq!(reply["code"], "validation");
        assert_eq!(reply["errors"][0]["field"], "term");
    }
}

#[tokio::test]
async fn tmdb_failures_are_502_and_never_show_the_key() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/search/movie", json_body(INVALID_KEY, 401), 1).await;
    let (status, reply) = lookup(&app, "matrix").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(reply["code"], "tmdb_key_rejected");
    assert!(!reply.to_string().contains(KEY));

    let server = MockServer::start().await;
    let app = configured(&server, "/3/search/movie", ResponseTemplate::new(500), 1).await;
    let (status, reply) = lookup(&app, "matrix").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(reply["code"], "tmdb_unavailable");
    assert!(!reply.to_string().contains(KEY));
}

#[tokio::test]
async fn lookup_needs_library_add() {
    let server = MockServer::start().await;
    let app = configured(&server, "/3/search/movie", json_body(SEARCH, 200), 1).await;

    let (status, _, reply) = app
        .send(Method::GET, &format!("{LOOKUP}?term=x"), None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{reply}");

    let (_, reader, _) = app.member("reader@example.com", &["library:read"]).await;
    let (status, _, reply) = app
        .send(
            Method::GET,
            &format!("{LOOKUP}?term=x"),
            Some(&reader),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["requiredPermission"], "library:add");

    let (_, adder, _) = app.member("adder@example.com", &["library:add"]).await;
    let (status, _, reply) = app
        .send(Method::GET, &format!("{LOOKUP}?term=x"), Some(&adder), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
}

#[tokio::test]
async fn settings_store_the_key_encrypted_and_never_return_it() {
    let server = MockServer::start().await;
    let app = App::new(&server).await;

    let (status, reply) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reply,
        json!({ "hasApiKey": false, "apiKeyMustBeReentered": false, "language": "en-US" })
    );

    let (status, reply) = app
        .owner(
            Method::PUT,
            SETTINGS,
            Some(json!({ "apiKey": format!("  {KEY} "), "language": "fr-FR" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        reply,
        json!({ "hasApiKey": true, "apiKeyMustBeReentered": false, "language": "fr-FR" })
    );
    assert!(!reply.to_string().contains(KEY));

    // The stored value is not the key, and the audit log has the fact only.
    let stored: String = db::settings::get_string(&app.state.db, "tmdb.language")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored, "fr-FR");
    let (_, log) = app
        .owner(
            Method::GET,
            "/api/v1/audit-log?action=setting.updated",
            None,
        )
        .await;
    assert_eq!(log["total"], 1);
    assert_eq!(log["items"][0]["targetKind"], "setting");
    assert_eq!(log["items"][0]["targetId"], "tmdb");
    assert_eq!(log["items"][0]["data"]["apiKey"], "changed");
    assert_eq!(log["items"][0]["data"]["language"]["new"], "fr-FR");
    assert!(!log.to_string().contains(KEY));

    // A change without a key keeps the stored key.
    let (status, reply) = app
        .owner(Method::PUT, SETTINGS, Some(json!({ "language": "de-DE" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["hasApiKey"], true);
    assert_eq!(reply["language"], "de-DE");
}

#[tokio::test]
async fn lookup_uses_the_language_setting() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/3/search/movie"))
        .and(query_param("api_key", KEY))
        .and(query_param("language", "fr-FR"))
        .respond_with(json_body(SEARCH, 200))
        .expect(1)
        .mount(&server)
        .await;
    let app = App::new(&server).await;
    let (status, reply) = app
        .owner(
            Method::PUT,
            SETTINGS,
            Some(json!({ "apiKey": KEY, "language": "fr-FR" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    let (status, _) = lookup(&app, "matrix").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn invalid_settings_are_refused() {
    let server = MockServer::start().await;
    let app = App::new(&server).await;
    for (body, field) in [
        (json!({ "apiKey": "   " }), "apiKey"),
        (json!({ "language": "english" }), "language"),
    ] {
        let (status, reply) = app.owner(Method::PUT, SETTINGS, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["errors"][0]["field"], field);
    }
    let (_, reply) = app.owner(Method::GET, SETTINGS, None).await;
    assert_eq!(reply["hasApiKey"], false);
}

#[tokio::test]
async fn settings_need_settings_read_and_manage() {
    let server = MockServer::start().await;
    let app = App::new(&server).await;
    let (_, reader, _) = app.member("reader@example.com", &["settings:read"]).await;
    let (_, none, _) = app.member("none@example.com", &[]).await;

    let (status, _, _) = app.send(Method::GET, SETTINGS, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app.send(Method::GET, SETTINGS, Some(&none), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, reply) = app
        .send(
            Method::PUT,
            SETTINGS,
            Some(&reader),
            Some(json!({ "language": "fr-FR" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(reply["requiredPermission"], "settings:manage");
    let (status, _, _) = app
        .send(
            Method::PUT,
            SETTINGS,
            None,
            Some(json!({ "language": "fr-FR" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
