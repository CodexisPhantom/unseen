//! Integration and contract tests for Radarr v3 compat movie read endpoints:
//! `GET /movie`, `GET /movie?tmdbId=`, `GET /movie/{id}`, and
//! `GET /movie/lookup?term=tmdb:{id}` (issue #77, docs/14-compatibility.md).

use std::path::PathBuf;

use api::{AppState, router};
use arr_compat::radarr::RADARR_VERSION;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, Set};
use db::entities::{
    collections, movie_files, movie_profiles, movie_tags, movies, quality_profiles,
    role_permissions, roles, root_folders, tags, user_roles,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use unseen_core::quality::Quality;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";
const TMDB_KEY: &str = "fixture-key-0123456789abcdef";
const TMDB_MOVIE_603: &str = include_str!("../../../fixtures/tmdb/movie_603.json");
const TMDB_NOT_FOUND: &str = include_str!("../../../fixtures/tmdb/error_not_found.json");

const NULL_OBJECTS: [&str; 2] = ["MovieResource.movieFile", "MovieResource.collection"];

struct Reply {
    status: StatusCode,
    content_type: String,
    body: Value,
}

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

impl App {
    async fn new(tmdb_url: Option<String>) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let mut state = AppState::new(db, "test secret", None).await.unwrap();
        if let Some(url) = tmdb_url {
            state.tmdb_base_url = url;
        }
        let router = router(state.clone());
        let mut app = Self {
            router,
            state,
            owner: String::new(),
            _dir: dir,
        };
        let setup = json!({
            "name": "Owner",
            "email": "owner@example.com",
            "password": PASSWORD,
        });
        let response = app
            .router
            .clone()
            .oneshot(
                Request::post("/api/v1/setup")
                    .header(header::HOST, HOST)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(setup.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        app.owner = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .find(|cookie| cookie.contains("session_token="))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        app
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            content_type,
            body,
        }
    }

    async fn member(&self, email: &str, permissions: &[&str]) -> String {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let response = self
            .router
            .clone()
            .oneshot(
                Request::post("/api/auth/sign-up/email")
                    .header(header::HOST, HOST)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .find(|cookie| cookie.contains("session_token="))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        let user_id = payload["user"]["id"].as_str().unwrap().to_owned();

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
        reply.body["key"].as_str().unwrap().to_owned()
    }

    async fn configure_tmdb_key(&self) {
        db::settings::set_secret(
            &self.state.db,
            &self.state.secrets,
            "tmdb.api_key",
            &db::secret::Secret::new(TMDB_KEY),
        )
        .await
        .unwrap();
    }
}

fn req(uri: &str, api_key: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::HOST, HOST)
        .header("X-Api-Key", api_key)
        .body(Body::empty())
        .unwrap()
}

fn json_body(body: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

struct SchemaChecker {
    document: Value,
}

impl SchemaChecker {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/radarr")
            .join(format!("openapi-{RADARR_VERSION}.json"));
        let text = std::fs::read_to_string(path).expect("pinned radarr openapi schema");
        Self {
            document: serde_json::from_str(&text).expect("valid json"),
        }
    }

    fn schema(&self, name: &str) -> &Value {
        &self.document["components"]["schemas"][name]
    }

    fn problems(&self, name: &str, value: &Value, require_all_fields: bool) -> Vec<String> {
        let mut found = Vec::new();
        self.check(
            self.schema(name),
            value,
            name,
            name,
            require_all_fields,
            &mut found,
        );
        found
    }

    fn check(
        &self,
        schema: &Value,
        value: &Value,
        owner: &str,
        path: &str,
        require_all_fields: bool,
        found: &mut Vec<String>,
    ) {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference.rsplit('/').next().unwrap();
            return self.check(
                self.schema(name),
                value,
                name,
                path,
                require_all_fields,
                found,
            );
        }
        if value.is_null() {
            if schema["nullable"] != true {
                found.push(format!("{path}: null where schema is not nullable"));
            }
            return;
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                found.push(format!("{path}: {value} is not in enum {allowed:?}"));
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
                Some("int32") => value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
                _ => value.is_i64() || value.is_u64(),
            },
            Some("array") => value.is_array(),
            Some("object") => value.is_object(),
            Some(other) => panic!("{path}: unexpected schema type {other}"),
        };
        if !fits {
            found.push(format!(
                "{path}: {value} does not match {kind:?} {format:?}"
            ));
            return;
        }
        match (kind, format, value) {
            (Some("string"), Some("date-time"), Value::String(text))
                if chrono::DateTime::parse_from_rfc3339(text).is_err() =>
            {
                found.push(format!("{path}: {text:?} is not RFC 3339"));
            }
            (Some("array"), _, Value::Array(items)) => {
                for (index, item) in items.iter().enumerate() {
                    self.check(
                        &schema["items"],
                        item,
                        owner,
                        &format!("{path}[{index}]"),
                        require_all_fields,
                        found,
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
                        None if require_all_fields => {
                            found.push(format!("{path}.{key}: missing"));
                        }
                        None => {}
                        Some(Value::Null)
                            if NULL_OBJECTS.contains(&format!("{owner}.{key}").as_str()) => {}
                        Some(inner) => self.check(
                            sub,
                            inner,
                            owner,
                            &format!("{path}.{key}"),
                            require_all_fields,
                            found,
                        ),
                    }
                }
                if schema["additionalProperties"] == false {
                    for key in map.keys().filter(|key| !properties.contains_key(*key)) {
                        found.push(format!("{path}.{key}: not in schema"));
                    }
                }
            }
            _ => {}
        }
    }
}

async fn seed_profile(app: &App, name: &str) -> i32 {
    quality_profiles::ActiveModel {
        name: Set(name.to_owned()),
        upgrade_allowed: Set(true),
        cutoff_quality_id: Set(Quality::Bluray1080p.id()),
        items: Set(json!([])),
        language: Set("en".to_owned()),
        min_format_score: Set(0),
        cutoff_format_score: Set(0),
        min_upgrade_format_score: Set(1),
        created_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap()
    .id
}

async fn seed_root_folder(app: &App, folder_path: &str) -> i32 {
    root_folders::ActiveModel {
        path: Set(folder_path.to_owned()),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn list_get_and_filter_movies_for_seerr_library_scan() {
    let app = App::new(None).await;
    let key = app.create_key(&app.owner, &["library:read"]).await;
    let checker = SchemaChecker::new();

    // Empty library returns [].
    let empty = app.send(req("/radarr/api/v3/movie", &key)).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.body, json!([]));

    let root = seed_root_folder(&app, "/media/movies").await;
    let primary_qp = seed_profile(&app, "Custom-1080p").await;
    let secondary_qp = seed_profile(&app, "Custom-4K").await;

    let collection = collections::ActiveModel {
        tmdb_id: Set(2344),
        title: Set("The Matrix Collection".to_owned()),
        sort_title: Set("matrix collection".to_owned()),
        monitored: Set(false),
        minimum_availability: Set("released".to_owned()),
        added_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let tag = tags::ActiveModel {
        label: Set("seerr-user".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    // Adopted movie with primary profile file, secondary profile file, and an
    // unmanaged file: compat API must expose only the primary profile + file.
    let adopted = movies::ActiveModel {
        tmdb_id: Set(603),
        imdb_id: Set(Some("tt0133093".to_owned())),
        title: Set("The Matrix".to_owned()),
        original_title: Set(Some("The Matrix".to_owned())),
        sort_title: Set("matrix".to_owned()),
        year: Set(1999),
        overview: Set(Some("A hacker learns the truth.".to_owned())),
        runtime: Set(Some(136)),
        status: Set("released".to_owned()),
        in_cinemas: Set(Some("1999-03-30".to_owned())),
        digital_release: Set(Some("2004-09-21T00:00:00Z".to_owned())),
        physical_release: Set(None),
        poster_path: Set(Some("/poster.jpg".to_owned())),
        collection_id: Set(Some(collection.id)),
        root_folder_id: Set(root),
        folder_name: Set("The Matrix (1999)".to_owned()),
        monitored: Set(true),
        minimum_availability: Set("released".to_owned()),
        added_at: Set("2026-11-16T20:15:13Z".to_owned()),
        updated_at: Set("2026-11-16T20:15:13Z".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let primary_link = movie_profiles::ActiveModel {
        movie_id: Set(adopted.id),
        quality_profile_id: Set(primary_qp),
        is_primary: Set(true),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let secondary_link = movie_profiles::ActiveModel {
        movie_id: Set(adopted.id),
        quality_profile_id: Set(secondary_qp),
        is_primary: Set(false),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let primary_file = movie_files::ActiveModel {
        movie_id: Set(adopted.id),
        relative_path: Set("The Matrix (1999) Bluray-1080p.mkv".to_owned()),
        size: Set(8_000_000_000),
        quality_id: Set(Quality::Bluray1080p.id()),
        release_group: Set(Some("SPARKS".to_owned())),
        edition: Set(None),
        scene_name: Set(Some("The.Matrix.1999.1080p.BluRay.x264-SPARKS".to_owned())),
        media_info: Set(None),
        languages: Set(Some(json!(["en"]))),
        movie_profile_id: Set(Some(primary_link.id)),
        added_at: Set("2026-11-17T08:00:00Z".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    // Secondary profile file and unmanaged file should not leak into compat.
    movie_files::ActiveModel {
        movie_id: Set(adopted.id),
        relative_path: Set("The Matrix (1999) Remux-2160p.mkv".to_owned()),
        size: Set(50_000_000_000),
        quality_id: Set(Quality::Remux2160p.id()),
        movie_profile_id: Set(Some(secondary_link.id)),
        added_at: Set("2026-11-17T09:00:00Z".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    movie_files::ActiveModel {
        movie_id: Set(adopted.id),
        relative_path: Set("extra.mkv".to_owned()),
        size: Set(1_000_000),
        quality_id: Set(Quality::Unknown.id()),
        movie_profile_id: Set(None),
        added_at: Set("2026-11-17T09:30:00Z".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    movie_tags::ActiveModel {
        movie_id: Set(adopted.id),
        tag_id: Set(tag.id),
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    // Second movie: unreleased, no file, no collection.
    let unreleased = movies::ActiveModel {
        tmdb_id: Set(999_001),
        title: Set("Future Film".to_owned()),
        sort_title: Set("future film".to_owned()),
        year: Set(2035),
        status: Set("announced".to_owned()),
        in_cinemas: Set(Some("2035-06-01".to_owned())),
        digital_release: Set(Some("2035-09-01".to_owned())),
        root_folder_id: Set(root),
        folder_name: Set("Future Film (2035)".to_owned()),
        monitored: Set(true),
        minimum_availability: Set("released".to_owned()),
        added_at: Set("2026-11-20T12:00:00Z".to_owned()),
        updated_at: Set("2026-11-20T12:00:00Z".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    movie_profiles::ActiveModel {
        movie_id: Set(unreleased.id),
        quality_profile_id: Set(primary_qp),
        is_primary: Set(true),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    // 1. GET /movie (Seerr library scan, including ignored options).
    let list = app
        .send(req("/radarr/api/v3/Movie?excludeLocalCovers=true", &key))
        .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let items = list.body.as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        let problems = checker.problems("MovieResource", item, true);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    // Sorted by sort_title: "future film" before "matrix".
    let future_json = &items[0];
    assert_eq!(future_json["id"], unreleased.id);
    assert_eq!(future_json["tmdbId"], 999_001);
    assert_eq!(future_json["isAvailable"], false);
    assert_eq!(future_json["hasFile"], false);
    assert_eq!(future_json["movieFile"], Value::Null);
    assert_eq!(future_json["collection"], Value::Null);

    let matrix_json = &items[1];
    assert_eq!(matrix_json["id"], adopted.id);
    assert_eq!(matrix_json["tmdbId"], 603);
    assert_eq!(matrix_json["imdbId"], "tt0133093");
    assert_eq!(matrix_json["titleSlug"], "603");
    assert_eq!(matrix_json["isAvailable"], true);
    assert_eq!(matrix_json["hasFile"], true);
    assert_eq!(matrix_json["qualityProfileId"], primary_qp);
    assert_eq!(matrix_json["movieFileId"], primary_file.id);
    assert_eq!(matrix_json["sizeOnDisk"], 8_000_000_000_i64);
    assert_eq!(matrix_json["path"], "/media/movies/The Matrix (1999)");
    assert_eq!(matrix_json["folderName"], "/media/movies/The Matrix (1999)");
    assert_eq!(matrix_json["rootFolderPath"], "/media/movies");
    assert_eq!(matrix_json["tags"], json!([tag.id]));
    assert_eq!(
        matrix_json["collection"],
        json!({ "title": "The Matrix Collection", "tmdbId": 2344 })
    );
    assert_eq!(matrix_json["movieFile"]["id"], primary_file.id);
    assert_eq!(matrix_json["movieFile"]["movieId"], adopted.id);
    assert_eq!(
        matrix_json["movieFile"]["relativePath"],
        "The Matrix (1999) Bluray-1080p.mkv"
    );
    assert_eq!(
        matrix_json["movieFile"]["path"],
        "/media/movies/The Matrix (1999)/The Matrix (1999) Bluray-1080p.mkv"
    );
    assert_eq!(
        matrix_json["movieFile"]["quality"]["quality"]["name"],
        "Bluray-1080p"
    );
    assert_eq!(matrix_json["movieFile"]["languages"][0]["id"], 1);
    assert_eq!(matrix_json["movieFile"]["languages"][0]["name"], "English");

    // 2. GET /movie?tmdbId=603 and ?tmdbId=404404.
    let by_tmdb = app.send(req("/radarr/api/v3/movie?tmdbId=603", &key)).await;
    assert_eq!(by_tmdb.status, StatusCode::OK);
    assert_eq!(by_tmdb.body.as_array().unwrap().len(), 1);
    assert_eq!(by_tmdb.body[0]["id"], adopted.id);
    assert!(
        checker
            .problems("MovieResource", &by_tmdb.body[0], true)
            .is_empty()
    );

    let missing_tmdb = app
        .send(req("/radarr/api/v3/movie?tmdbId=404404", &key))
        .await;
    assert_eq!(missing_tmdb.status, StatusCode::OK);
    assert_eq!(missing_tmdb.body, json!([]));

    let bad_tmdb = app
        .send(req("/radarr/api/v3/movie?tmdbId=notanumber", &key))
        .await;
    assert_eq!(bad_tmdb.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_tmdb.body[0]["propertyName"], "tmdbId");

    // 3. GET /movie/{id}.
    let single = app
        .send(req(&format!("/radarr/api/v3/movie/{}", adopted.id), &key))
        .await;
    assert_eq!(single.status, StatusCode::OK);
    assert_eq!(single.body["id"], adopted.id);
    assert_eq!(single.body["tmdbId"], 603);
    assert!(
        checker
            .problems("MovieResource", &single.body, true)
            .is_empty()
    );

    let not_found = app.send(req("/radarr/api/v3/movie/99999", &key)).await;
    assert_eq!(not_found.status, StatusCode::NOT_FOUND);
    assert_eq!(not_found.body, json!({ "message": "NotFound" }));

    let invalid_id = app.send(req("/radarr/api/v3/movie/abc", &key)).await;
    assert_eq!(invalid_id.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_id.content_type, "application/json");
}

#[tokio::test]
async fn lookup_returns_library_movie_with_id_or_tmdb_movie_without_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/3/movie/603"))
        .and(query_param("api_key", TMDB_KEY))
        .respond_with(json_body(TMDB_MOVIE_603, 200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/3/movie/1"))
        .and(query_param("api_key", TMDB_KEY))
        .respond_with(json_body(TMDB_NOT_FOUND, 404))
        .expect(1)
        .mount(&server)
        .await;

    let app = App::new(Some(server.uri())).await;
    app.configure_tmdb_key().await;
    let key = app.create_key(&app.owner, &["library:read"]).await;
    let checker = SchemaChecker::new();

    // 1. Outside the library: returns TMDB metadata WITHOUT `id`.
    let external = app
        .send(req("/radarr/api/v3/movie/lookup?term=tmdb:603", &key))
        .await;
    assert_eq!(external.status, StatusCode::OK, "{}", external.body);
    let results = external.body.as_array().unwrap();
    assert_eq!(results.len(), 1);
    let movie = &results[0];
    assert!(
        movie.get("id").is_none(),
        "external lookup must omit id: {movie}"
    );
    assert_eq!(movie["tmdbId"], 603);
    assert_eq!(movie["imdbId"], "tt0133093");
    assert_eq!(movie["title"], "The Matrix");
    assert_eq!(movie["year"], 1999);
    assert_eq!(movie["hasFile"], false);
    assert_eq!(movie["movieFile"], Value::Null);
    assert_eq!(movie["isAvailable"], true);
    assert_eq!(movie["path"], "");
    assert_eq!(movie["folderName"], "");
    assert_eq!(movie["added"], "0001-01-01T00:00:00Z");
    assert_eq!(
        movie["collection"],
        json!({ "title": "The Matrix Collection", "tmdbId": 2344 })
    );
    assert!(
        checker.problems("MovieResource", movie, false).is_empty(),
        "{:#?}",
        checker.problems("MovieResource", movie, false)
    );
    assert_eq!(
        checker.problems("MovieResource", movie, true),
        ["MovieResource.id: missing"]
    );

    // 2. TMDB 404 returns [].
    let not_found = app
        .send(req("/radarr/api/v3/movie/lookup?term=tmdb:1", &key))
        .await;
    assert_eq!(not_found.status, StatusCode::OK);
    assert_eq!(not_found.body, json!([]));

    // 3. Add the movie to the library; subsequent lookup returns the library
    //    record WITH its `id` without hitting TMDB again (expect(1) above).
    let root = seed_root_folder(&app, "/media/movies").await;
    let qp = seed_profile(&app, "Custom-Lookup-1080p").await;
    let stored = movies::ActiveModel {
        tmdb_id: Set(603),
        imdb_id: Set(Some("tt0133093".to_owned())),
        title: Set("The Matrix".to_owned()),
        sort_title: Set("matrix".to_owned()),
        year: Set(1999),
        status: Set("released".to_owned()),
        in_cinemas: Set(Some("1999-03-30".to_owned())),
        digital_release: Set(Some("2004-09-21".to_owned())),
        root_folder_id: Set(root),
        folder_name: Set("The Matrix (1999)".to_owned()),
        monitored: Set(true),
        minimum_availability: Set("released".to_owned()),
        added_at: Set("2026-11-16T20:15:13Z".to_owned()),
        updated_at: Set("2026-11-16T20:15:13Z".to_owned()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
    movie_profiles::ActiveModel {
        movie_id: Set(stored.id),
        quality_profile_id: Set(qp),
        is_primary: Set(true),
        created_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();

    let in_library = app
        .send(req("/Radarr/API/v3/Movie/Lookup?term=TMDB:603", &key))
        .await;
    assert_eq!(in_library.status, StatusCode::OK, "{}", in_library.body);
    let results = in_library.body.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], stored.id);
    assert_eq!(results[0]["qualityProfileId"], qp);
    assert_eq!(results[0]["path"], "/media/movies/The Matrix (1999)");
    assert!(
        checker
            .problems("MovieResource", &results[0], true)
            .is_empty()
    );

    // 4. Text lookup is out of scope (501), malformed/empty term is 400.
    let text_lookup = app
        .send(req("/radarr/api/v3/movie/lookup?term=matrix", &key))
        .await;
    assert_eq!(text_lookup.status, StatusCode::NOT_IMPLEMENTED);

    for bad_uri in [
        "/radarr/api/v3/movie/lookup",
        "/radarr/api/v3/movie/lookup?term=",
        "/radarr/api/v3/movie/lookup?term=tmdb:abc",
    ] {
        let bad = app.send(req(bad_uri, &key)).await;
        assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{bad_uri}");
        assert_eq!(bad.body[0]["propertyName"], "term", "{bad_uri}");
    }
}

#[tokio::test]
async fn compat_movie_endpoints_require_library_read_permission() {
    let app = App::new(None).await;
    let cookie = app.member("noperms@example.com", &["profiles:read"]).await;
    let wrong_key = app.create_key(&cookie, &["profiles:read"]).await;

    for uri in [
        "/radarr/api/v3/movie",
        "/radarr/api/v3/movie?tmdbId=603",
        "/radarr/api/v3/movie/1",
        "/radarr/api/v3/movie/lookup?term=tmdb:603",
    ] {
        let forbidden = app.send(req(uri, &wrong_key)).await;
        assert_eq!(forbidden.status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(
            forbidden.body["requiredPermission"], "library:read",
            "{uri}"
        );

        let unauth = app
            .send(
                Request::get(uri)
                    .header(header::HOST, HOST)
                    .header(header::COOKIE, &app.owner)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(unauth.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}
