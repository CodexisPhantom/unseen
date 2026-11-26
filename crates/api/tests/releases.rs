use std::time::Duration;

use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::entities::{custom_formats, indexers, movie_files, role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use unseen_core::search::SearchCache;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";
const TMDB_KEY: &str = "fixture-tmdb-key-0123456789";

const MOVIE_603: &str = include_str!("../../../fixtures/tmdb/movie_603.json");
const JACKETT_CAPS: &str = include_str!("../../../fixtures/torznab/jackett_caps.xml");
const ERROR_CREDENTIALS: &str = include_str!("../../../fixtures/torznab/error_credentials.xml");

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

impl App {
    async fn with_cache(tmdb: &MockServer, cache: SearchCache) -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let mut state = AppState::new(db, "test secret", None).await.unwrap();
        state.tmdb_base_url = tmdb.uri();
        state.search_cache = cache;
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
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        cookie.unwrap()
    }
}

struct Fixture {
    app: App,
    root_id: i64,
    hd_profile_id: i64,
    uhd_profile_id: i64,
    _root_dir: TempDir,
}

impl Fixture {
    async fn new(tmdb: &MockServer) -> Self {
        Self::with_cache(tmdb, SearchCache::new()).await
    }

    async fn with_cache(tmdb: &MockServer, cache: SearchCache) -> Self {
        Mock::given(method("GET"))
            .and(path("/3/movie/603"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(MOVIE_603.as_bytes().to_vec(), "application/json"),
            )
            .mount(tmdb)
            .await;

        let app = App::with_cache(tmdb, cache).await;
        db::settings::set_secret(
            &app.state.db,
            &app.state.secrets,
            "tmdb.api_key",
            &db::secret::Secret::new(TMDB_KEY),
        )
        .await
        .unwrap();
        let root_dir = tempfile::tempdir().unwrap();
        let (status, root) = app
            .owner(
                Method::POST,
                "/api/v1/root-folders",
                Some(json!({ "path": root_dir.path().to_str().unwrap() })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{root}");

        let (_, profiles) = app
            .owner(Method::GET, "/api/v1/quality-profiles", None)
            .await;
        let find_profile = |name: &str| {
            profiles
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == name)
                .unwrap()["id"]
                .as_i64()
                .unwrap()
        };

        Self {
            root_id: root["id"].as_i64().unwrap(),
            hd_profile_id: find_profile("HD-1080p"),
            uhd_profile_id: find_profile("Ultra-HD"),
            app,
            _root_dir: root_dir,
        }
    }

    async fn add_matrix(&self, profile_ids: &[i64]) -> Value {
        let (status, movie) = self
            .app
            .owner(
                Method::POST,
                "/api/v1/movies",
                Some(json!({
                    "tmdbId": 603,
                    "rootFolderId": self.root_id,
                    "qualityProfileIds": profile_ids,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{movie}");
        movie
    }

    async fn add_indexer(&self, payload: Value) -> i64 {
        let (status, reply) = self
            .app
            .owner(Method::POST, "/api/v1/indexers", Some(payload))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        reply["id"].as_i64().unwrap()
    }
}

fn xml(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "application/xml")
        .set_body_string(body)
}

struct MockItem<'a> {
    title: &'a str,
    guid: &'a str,
    size: u64,
    seeders: u32,
    peers: u32,
    download_url: Option<&'a str>,
    magnet_url: Option<&'a str>,
    info_hash: Option<&'a str>,
    pub_date: Option<&'a str>,
}

fn torznab_feed(items: &[MockItem<'_>]) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed">
  <channel>
"#,
    );
    for item in items {
        xml.push_str("    <item>\n");
        xml.push_str(&format!("      <title>{}</title>\n", item.title));
        xml.push_str(&format!("      <guid>{}</guid>\n", item.guid));
        xml.push_str(&format!("      <size>{}</size>\n", item.size));
        if let Some(pub_date) = item.pub_date {
            xml.push_str(&format!("      <pubDate>{pub_date}</pubDate>\n"));
        }
        if let Some(url) = item.download_url {
            xml.push_str(&format!(
                "      <enclosure url=\"{url}\" length=\"{}\" type=\"application/x-bittorrent\" />\n",
                item.size
            ));
        }
        if let Some(magnet) = item.magnet_url {
            xml.push_str(&format!(
                "      <torznab:attr name=\"magneturl\" value=\"{magnet}\" />\n"
            ));
        }
        if let Some(hash) = item.info_hash {
            xml.push_str(&format!(
                "      <torznab:attr name=\"infohash\" value=\"{hash}\" />\n"
            ));
        }
        xml.push_str(&format!(
            "      <torznab:attr name=\"seeders\" value=\"{}\" />\n",
            item.seeders
        ));
        xml.push_str(&format!(
            "      <torznab:attr name=\"peers\" value=\"{}\" />\n",
            item.peers
        ));
        xml.push_str("      <torznab:attr name=\"category\" value=\"2000\" />\n");
        xml.push_str("    </item>\n");
    }
    xml.push_str("  </channel>\n</rss>\n");
    xml
}

async fn mount_torznab(server: &MockServer, api_key: &str, feed: String) {
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .and(query_param("apikey", api_key))
        .respond_with(xml(JACKETT_CAPS))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("imdbid", "0133093"))
        .and(query_param("apikey", api_key))
        .respond_with(xml(&feed))
        .mount(server)
        .await;
}

const MIB: u64 = 1024 * 1024;

#[tokio::test]
async fn merges_and_ranks_results_from_two_indexers_and_skips_disabled() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();
    let movie_profile_id = movie["profiles"][0]["id"].as_i64().unwrap();

    let alpha_server = MockServer::start().await;
    let beta_server = MockServer::start().await;
    let disabled_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(xml(JACKETT_CAPS))
        .expect(0)
        .mount(&disabled_server)
        .await;

    let alpha_feed = torznab_feed(&[
        MockItem {
            title: "The.Matrix.1999.1080p.WEB-DL.x264-ALPHA",
            guid: "alpha-webdl",
            size: 6000 * MIB,
            seeders: 100,
            peers: 120,
            download_url: Some("http://alpha.test/dl/webdl?apikey=secret-alpha-key"),
            magnet_url: None,
            info_hash: Some("1111111111111111111111111111111111111111"),
            pub_date: None,
        },
        MockItem {
            title: "The.Matrix.1999.1080p.BluRay.x264-ALPHA",
            guid: "alpha-bluray",
            size: 10_000 * MIB,
            seeders: 50,
            peers: 60,
            download_url: Some("http://alpha.test/dl/bluray?apikey=secret-alpha-key"),
            magnet_url: Some("magnet:?xt=urn:btih:2222222222222222222222222222222222222222"),
            info_hash: Some("2222222222222222222222222222222222222222"),
            pub_date: Some("Sun, 22 Nov 2026 18:30:00 +0000"),
        },
        MockItem {
            title: "The.Matrix.1999.2160p.UHD.BluRay.x265-REJECT",
            guid: "alpha-uhd",
            size: 25_000 * MIB,
            seeders: 80,
            peers: 90,
            download_url: Some("http://alpha.test/dl/uhd"),
            magnet_url: None,
            info_hash: None,
            pub_date: None,
        },
    ]);
    let beta_feed = torznab_feed(&[
        MockItem {
            title: "The.Matrix.1999.IMAX.PROPER.1080p.BluRay.x264-BETA",
            guid: "beta-proper",
            size: 10_000 * MIB,
            seeders: 5,
            peers: 7,
            download_url: None,
            magnet_url: Some("magnet:?xt=urn:btih:3333333333333333333333333333333333333333"),
            info_hash: Some("3333333333333333333333333333333333333333"),
            pub_date: None,
        },
        MockItem {
            title: "The.Matrix.1999.1080p.BluRay.x264-BETA",
            guid: "beta-bluray",
            size: 10_000 * MIB,
            seeders: 500,
            peers: 520,
            download_url: Some("http://beta.test/dl/bluray"),
            magnet_url: None,
            info_hash: None,
            pub_date: None,
        },
        MockItem {
            title: "The.Matrix.1999.REPACK.1080p.HDTV.x264-BETA",
            guid: "beta-hdtv",
            size: 4000 * MIB,
            seeders: 20,
            peers: 25,
            download_url: Some("http://beta.test/dl/hdtv"),
            magnet_url: None,
            info_hash: None,
            pub_date: None,
        },
    ]);

    mount_torznab(&alpha_server, "secret-alpha-key", alpha_feed).await;
    mount_torznab(&beta_server, "secret-beta-key", beta_feed).await;

    let alpha_id = fx
        .add_indexer(json!({
            "name": "Alpha",
            "url": alpha_server.uri(),
            "apiKey": "secret-alpha-key",
            "categories": [2000],
            "priority": 10,
            "enabled": true,
        }))
        .await;
    let beta_id = fx
        .add_indexer(json!({
            "name": "Beta",
            "url": beta_server.uri(),
            "apiKey": "secret-beta-key",
            "categories": [2000],
            "priority": 25,
            "enabled": true,
        }))
        .await;
    fx.add_indexer(json!({
        "name": "Disabled",
        "url": disabled_server.uri(),
        "apiKey": "secret-disabled-key",
        "enabled": false,
    }))
    .await;

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["movieId"], movie_id);
    assert_eq!(reply["movieProfileId"], movie_profile_id);
    assert_eq!(reply["qualityProfileId"], fx.hd_profile_id);
    assert_eq!(reply["indexerErrors"], json!([]));

    // Neither API keys nor download URLs may leak in the HTTP response.
    let raw_json = reply.to_string();
    assert!(!raw_json.contains("secret-alpha-key"));
    assert!(!raw_json.contains("secret-beta-key"));
    assert!(!raw_json.contains("http://alpha.test/dl/"));

    let releases = reply["releases"].as_array().unwrap();
    let guids: Vec<&str> = releases
        .iter()
        .map(|r| r["guid"].as_str().unwrap())
        .collect();
    assert_eq!(
        guids,
        [
            "beta-proper",
            "alpha-bluray",
            "beta-bluray",
            "alpha-webdl",
            "beta-hdtv",
            "alpha-uhd",
        ]
    );

    // Check detailed fields on #1 (Beta PROPER BluRay).
    let first = &releases[0];
    assert_eq!(first["indexerId"], beta_id);
    assert_eq!(first["indexerName"], "Beta");
    assert_eq!(first["qualityId"], 7);
    assert_eq!(first["qualityName"], "Bluray-1080p");
    assert_eq!(first["protocol"], "torrent");
    assert_eq!(first["proper"], true);
    assert_eq!(first["repack"], false);
    assert_eq!(first["edition"], "IMAX");
    assert_eq!(first["releaseGroup"], "BETA");
    assert_eq!(first["seeders"], 5);
    assert_eq!(first["leechers"], 2);
    assert_eq!(first["accepted"], true);
    assert_eq!(first["rejections"], json!([]));
    assert_eq!(first["score"]["rank"], 1);
    assert_eq!(first["score"]["revision"], 2);
    assert_eq!(first["score"]["indexerPriority"], 25);
    assert_eq!(first["score"]["customFormatScore"], 0);

    // Check #2 (Alpha BluRay beats Beta BluRay via priority 10 < 25).
    let second = &releases[1];
    assert_eq!(second["indexerId"], alpha_id);
    assert_eq!(second["indexerName"], "Alpha");
    assert_eq!(second["leechers"], 10);
    assert_eq!(second["publishedAt"], "2026-11-22T18:30:00.000Z");
    assert!(second["ageHours"].as_f64().unwrap() >= 0.0);
    assert_eq!(second["score"]["rank"], 2);
    assert_eq!(second["score"]["indexerPriority"], 10);

    // Check #5 (Beta REPACK HDTV).
    assert_eq!(releases[4]["repack"], true);
    assert_eq!(releases[4]["qualityName"], "HDTV-1080p");
    assert_eq!(releases[4]["score"]["rank"], 5);

    // Check #6 (rejected UHD release placed after all accepted releases).
    let rejected = &releases[5];
    assert_eq!(rejected["accepted"], false);
    assert_eq!(rejected["score"]["rank"], 6);
    assert_eq!(rejected["rejections"][0]["code"], "quality_not_allowed");

    // Verify all returned release ids are cached with grab metadata.
    let mut ids = std::collections::BTreeSet::new();
    for item in releases {
        let id = item["id"].as_str().unwrap();
        assert!(ids.insert(id.to_owned()), "duplicate release id");
        let cached = fx.app.state.search_cache.get(id).expect("cached release");
        assert_eq!(cached.movie_id, movie_id as i32);
        assert_eq!(cached.movie_profile_id, movie_profile_id as i32);
        assert_eq!(cached.guid, item["guid"].as_str().unwrap());
        assert_eq!(cached.protocol, "torrent");
    }
    let cached_second = fx
        .app
        .state
        .search_cache
        .get(second["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        cached_second.download_url.as_deref(),
        Some("http://alpha.test/dl/bluray?apikey=secret-alpha-key")
    );
    assert_eq!(
        cached_second.magnet_url.as_deref(),
        Some("magnet:?xt=urn:btih:2222222222222222222222222222222222222222")
    );
    assert_eq!(
        cached_second.info_hash.as_deref(),
        Some("2222222222222222222222222222222222222222")
    );
}

#[tokio::test]
async fn one_failing_indexer_does_not_fail_the_request() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();

    let healthy = MockServer::start().await;
    let bad_key = MockServer::start().await;
    let unreachable = MockServer::start().await;
    let unreachable_uri = unreachable.uri();
    drop(unreachable);

    mount_torznab(
        &healthy,
        "good-key",
        torznab_feed(&[MockItem {
            title: "The.Matrix.1999.1080p.BluRay.x264-OK",
            guid: "ok-1",
            size: 8000 * MIB,
            seeders: 30,
            peers: 35,
            download_url: Some("http://healthy.test/dl/1"),
            magnet_url: None,
            info_hash: None,
            pub_date: None,
        }]),
    )
    .await;
    Mock::given(method("GET"))
        .respond_with(xml(ERROR_CREDENTIALS))
        .mount(&bad_key)
        .await;

    fx.add_indexer(json!({
        "name": "A-Healthy",
        "url": healthy.uri(),
        "apiKey": "good-key",
    }))
    .await;
    let bad_key_id = fx
        .add_indexer(json!({
            "name": "B-BadKey",
            "url": bad_key.uri(),
            "apiKey": "wrong-secret-token",
        }))
        .await;
    let unreachable_id = fx
        .add_indexer(json!({
            "name": "C-Down",
            "url": unreachable_uri,
            "apiKey": "down-secret-token",
        }))
        .await;
    let unreadable_id = fx
        .add_indexer(json!({
            "name": "D-Unreadable",
            "url": healthy.uri(),
            "apiKey": "temp-key",
        }))
        .await;

    // Corrupt the stored API key with a different root secret.
    let other_secret = db::secret::SecretKey::derive("other root secret");
    let foreign = db::secret::encrypt(&other_secret, &db::secret::Secret::new("secret"));
    let mut row: indexers::ActiveModel = indexers::Entity::find_by_id(unreadable_id as i32)
        .one(&fx.app.state.db)
        .await
        .unwrap()
        .unwrap()
        .into();
    row.api_key = Set(foreign);
    row.update(&fx.app.state.db).await.unwrap();

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["releases"].as_array().unwrap().len(), 1);
    assert_eq!(reply["releases"][0]["guid"], "ok-1");

    let errors = reply["indexerErrors"].as_array().unwrap();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert_eq!(errors[0]["indexerId"], bad_key_id);
    assert_eq!(errors[0]["indexerName"], "B-BadKey");
    assert!(errors[0]["message"].as_str().unwrap().contains("API key"));
    assert_eq!(errors[1]["indexerId"], unreachable_id);
    assert_eq!(errors[1]["indexerName"], "C-Down");
    assert!(!errors[1]["message"].as_str().unwrap().is_empty());
    assert_eq!(errors[2]["indexerId"], unreadable_id);
    assert_eq!(errors[2]["indexerName"], "D-Unreadable");
    assert!(
        errors[2]["message"]
            .as_str()
            .unwrap()
            .contains("cannot be read")
    );

    let text = reply.to_string();
    assert!(!text.contains("wrong-secret-token"));
    assert!(!text.contains("down-secret-token"));
}

#[tokio::test]
async fn misconfigured_proxy_fails_only_proxied_indexers() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();

    // Store an unreadable proxy URL encrypted with a different key.
    let other_secret = db::secret::SecretKey::derive("wrong secret");
    db::settings::set_secret(
        &fx.app.state.db,
        &other_secret,
        "proxy.url",
        &db::secret::Secret::new("http://user:pass@proxy.example:8080"),
    )
    .await
    .unwrap();

    let direct_server = MockServer::start().await;
    mount_torznab(
        &direct_server,
        "direct-key",
        torznab_feed(&[MockItem {
            title: "The.Matrix.1999.1080p.BluRay.x264-DIRECT",
            guid: "direct-1",
            size: 8000 * MIB,
            seeders: 15,
            peers: 20,
            download_url: Some("http://direct.test/dl/1"),
            magnet_url: None,
            info_hash: None,
            pub_date: None,
        }]),
    )
    .await;

    fx.add_indexer(json!({
        "name": "Direct",
        "url": direct_server.uri(),
        "apiKey": "direct-key",
        "useProxy": false,
    }))
    .await;
    let proxied_id = fx
        .add_indexer(json!({
            "name": "Proxied",
            "url": direct_server.uri(),
            "apiKey": "proxied-key",
            "useProxy": true,
        }))
        .await;

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["releases"].as_array().unwrap().len(), 1);
    assert_eq!(reply["releases"][0]["guid"], "direct-1");
    assert_eq!(reply["indexerErrors"].as_array().unwrap().len(), 1);
    assert_eq!(reply["indexerErrors"][0]["indexerId"], proxied_id);
    assert_eq!(reply["indexerErrors"][0]["indexerName"], "Proxied");
    assert!(
        reply["indexerErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("proxy")
    );
}

#[tokio::test]
async fn rejected_releases_report_exact_reasons() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;

    // Set size limits on Bluray-1080p (id 7): 20..=100 MiB/min.
    // The Matrix has runtime 136 min => 2720 MiB min, 13600 MiB max.
    let (status, reply) = fx
        .app
        .owner(
            Method::PATCH,
            "/api/v1/quality-definitions/7",
            Some(json!({ "minSize": 20.0, "preferredSize": 60.0, "maxSize": 100.0 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    // Create a quality profile requiring English and allowing Bluray-1080p & WEBDL-1080p.
    let (status, custom_profile) = fx
        .app
        .owner(
            Method::POST,
            "/api/v1/quality-profiles",
            Some(json!({
                "name": "English HD",
                "upgradeAllowed": true,
                "cutoff": 7,
                "language": "en",
                "items": [
                    { "type": "quality", "qualityId": 3, "allowed": true },
                    { "type": "quality", "qualityId": 7, "allowed": true },
                    { "type": "quality", "qualityId": 19, "allowed": false }
                ]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{custom_profile}");
    let english_profile_id = custom_profile["id"].as_i64().unwrap();

    let movie = fx.add_matrix(&[english_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();

    let (_, private_tag) = fx
        .app
        .owner(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "label": "private" })),
        )
        .await;
    let private_tag_id = private_tag["id"].as_i64().unwrap();

    let open_server = MockServer::start().await;
    let tagged_server = MockServer::start().await;

    mount_torznab(
        &open_server,
        "open-key",
        torznab_feed(&[
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-GOOD",
                guid: "good",
                size: 8000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/good"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.Reloaded.2003.1080p.BluRay.x264-WRONG",
                guid: "wrong-title",
                size: 8000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/wrong-title"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.2005.1080p.BluRay.x264-YEAR",
                guid: "wrong-year",
                size: 8000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/wrong-year"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.2160p.UHD.BluRay.x265-UHD",
                guid: "disallowed-quality",
                size: 8000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/uhd"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-TINY",
                guid: "too-small",
                size: 500 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/tiny"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-HUGE",
                guid: "too-large",
                size: 20_000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/huge"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.FRENCH.1080p.BluRay.x264-FR",
                guid: "wrong-lang",
                size: 8000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://open.test/fr"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
        ]),
    )
    .await;

    mount_torznab(
        &tagged_server,
        "tagged-key",
        torznab_feed(&[
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-TAGGED",
                guid: "tag-mismatch",
                size: 8000 * MIB,
                seeders: 50,
                peers: 55,
                download_url: Some("http://tagged.test/1"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "Other.Movie.2001.DVDRip.XviD-MULTI",
                guid: "multi-reject",
                size: 700 * MIB,
                seeders: 5,
                peers: 5,
                download_url: Some("http://tagged.test/2"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
        ]),
    )
    .await;

    fx.add_indexer(json!({
        "name": "Open",
        "url": open_server.uri(),
        "apiKey": "open-key",
    }))
    .await;
    fx.add_indexer(json!({
        "name": "Private",
        "url": tagged_server.uri(),
        "apiKey": "tagged-key",
        "tags": [private_tag_id],
    }))
    .await;

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    let releases = reply["releases"].as_array().unwrap();
    // Accepted release comes first.
    assert_eq!(releases[0]["guid"], "good");
    assert_eq!(releases[0]["accepted"], true);

    let codes_of = |guid: &str| -> Vec<&str> {
        let entry = releases
            .iter()
            .find(|r| r["guid"] == guid)
            .unwrap_or_else(|| panic!("missing {guid}"));
        assert_eq!(entry["accepted"], false, "{guid} should be rejected");
        let list = entry["rejections"].as_array().unwrap();
        for item in list {
            assert!(!item["reason"].as_str().unwrap().is_empty());
        }
        list.iter()
            .map(|item| item["code"].as_str().unwrap())
            .collect()
    };

    assert_eq!(codes_of("wrong-title"), ["wrong_movie"]);
    assert_eq!(codes_of("wrong-year"), ["wrong_movie"]);
    assert_eq!(codes_of("disallowed-quality"), ["quality_not_allowed"]);
    assert_eq!(codes_of("too-small"), ["size_too_small"]);
    assert_eq!(codes_of("too-large"), ["size_too_large"]);
    assert_eq!(codes_of("wrong-lang"), ["language_not_allowed"]);
    assert_eq!(codes_of("tag-mismatch"), ["indexer_tags_mismatch"]);
    assert_eq!(
        codes_of("multi-reject"),
        [
            "wrong_movie",
            "quality_not_allowed",
            "indexer_tags_mismatch"
        ]
    );

    let french = releases.iter().find(|r| r["guid"] == "wrong-lang").unwrap();
    assert_eq!(french["languages"], json!(["fr"]));
}

#[tokio::test]
async fn existing_file_rules_for_cutoff_not_an_upgrade_and_upgrades_disabled() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap() as i32;
    let movie_profile_id = movie["profiles"][0]["id"].as_i64().unwrap() as i32;

    let server = MockServer::start().await;
    mount_torznab(
        &server,
        "key",
        torznab_feed(&[
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.REMUX.AVC-GRP",
                guid: "remux",
                size: 25_000 * MIB,
                seeders: 10,
                peers: 12,
                download_url: Some("http://idx.test/remux"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.PROPER.1080p.BluRay.x264-GRP",
                guid: "bluray-proper",
                size: 10_000 * MIB,
                seeders: 10,
                peers: 12,
                download_url: Some("http://idx.test/bluray-proper"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-GRP",
                guid: "bluray",
                size: 10_000 * MIB,
                seeders: 10,
                peers: 12,
                download_url: Some("http://idx.test/bluray"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.WEB-DL.x264-GRP",
                guid: "webdl",
                size: 6000 * MIB,
                seeders: 10,
                peers: 12,
                download_url: Some("http://idx.test/webdl"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.HDTV.x264-GRP",
                guid: "hdtv",
                size: 4000 * MIB,
                seeders: 10,
                peers: 12,
                download_url: Some("http://idx.test/hdtv"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
        ]),
    )
    .await;
    fx.add_indexer(json!({
        "name": "Idx",
        "url": server.uri(),
        "apiKey": "key",
    }))
    .await;

    // 1. Existing managed file at WEBDL-1080p (id 3), below Bluray-1080p cutoff.
    let file = movie_files::ActiveModel {
        movie_id: Set(movie_id),
        relative_path: Set("The Matrix (1999) WEBDL-1080p.mkv".into()),
        size: Set(6_000_000_000),
        quality_id: Set(3),
        movie_profile_id: Set(Some(movie_profile_id)),
        added_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&fx.app.state.db)
    .await
    .unwrap();

    async fn search(app: &App, movie_id: i32) -> Value {
        let (status, reply) = app
            .owner(
                Method::GET,
                &format!("/api/v1/movies/{movie_id}/releases"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        reply
    }

    let by_guid = |reply: &Value, guid: &str| -> Value {
        reply["releases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["guid"] == guid)
            .unwrap()
            .clone()
    };

    let reply = search(&fx.app, movie_id).await;
    assert_eq!(by_guid(&reply, "bluray")["accepted"], true);
    assert_eq!(
        by_guid(&reply, "webdl")["rejections"][0]["code"],
        "not_an_upgrade"
    );
    assert_eq!(
        by_guid(&reply, "hdtv")["rejections"][0]["code"],
        "not_an_upgrade"
    );

    // 2. Existing file upgraded to Bluray-1080p (id 7), meeting the cutoff.
    let mut active: movie_files::ActiveModel = file.into();
    active.quality_id = Set(7);
    active.relative_path = Set("The Matrix (1999) Bluray-1080p.mkv".into());
    let file = active.update(&fx.app.state.db).await.unwrap();

    let reply = search(&fx.app, movie_id).await;
    assert_eq!(by_guid(&reply, "bluray-proper")["accepted"], true);
    assert_eq!(
        by_guid(&reply, "bluray")["rejections"][0]["code"],
        "cutoff_met"
    );
    assert_eq!(
        by_guid(&reply, "remux")["rejections"][0]["code"],
        "cutoff_met"
    );

    // 3. Disable upgrades on the quality profile, with existing file below cutoff.
    let mut active: movie_files::ActiveModel = file.into();
    active.quality_id = Set(3);
    active.update(&fx.app.state.db).await.unwrap();
    let (status, patched) = fx
        .app
        .owner(
            Method::PATCH,
            &format!("/api/v1/quality-profiles/{}", fx.hd_profile_id),
            Some(json!({ "upgradeAllowed": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");

    let reply = search(&fx.app, movie_id).await;
    assert_eq!(
        by_guid(&reply, "bluray")["rejections"][0]["code"],
        "upgrades_disabled"
    );
}

#[tokio::test]
async fn profile_id_selects_non_primary_profile_and_rejects_invalid() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id, fx.uhd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();
    let primary_mp = movie["profiles"][0]["id"].as_i64().unwrap();
    let secondary_mp = movie["profiles"][1]["id"].as_i64().unwrap();

    let server = MockServer::start().await;
    mount_torznab(
        &server,
        "key",
        torznab_feed(&[
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-HD",
                guid: "hd",
                size: 8000 * MIB,
                seeders: 40,
                peers: 45,
                download_url: Some("http://idx.test/hd"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.2160p.UHD.BluRay.x265-UHD",
                guid: "uhd",
                size: 25_000 * MIB,
                seeders: 40,
                peers: 45,
                download_url: Some("http://idx.test/uhd"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
        ]),
    )
    .await;
    fx.add_indexer(json!({
        "name": "Idx",
        "url": server.uri(),
        "apiKey": "key",
    }))
    .await;

    // Default uses primary profile (HD-1080p).
    let (status, default_reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{default_reply}");
    assert_eq!(default_reply["movieProfileId"], primary_mp);
    assert_eq!(default_reply["qualityProfileId"], fx.hd_profile_id);
    assert_eq!(default_reply["releases"][0]["guid"], "hd");
    assert_eq!(default_reply["releases"][0]["accepted"], true);
    assert_eq!(default_reply["releases"][1]["guid"], "uhd");
    assert_eq!(default_reply["releases"][1]["accepted"], false);

    // Selecting the non-primary profile (Ultra-HD) flips acceptance.
    let (status, uhd_reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases?profileId={secondary_mp}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{uhd_reply}");
    assert_eq!(uhd_reply["movieProfileId"], secondary_mp);
    assert_eq!(uhd_reply["qualityProfileId"], fx.uhd_profile_id);
    assert_eq!(uhd_reply["releases"][0]["guid"], "uhd");
    assert_eq!(uhd_reply["releases"][0]["accepted"], true);
    assert_eq!(uhd_reply["releases"][1]["guid"], "hd");
    assert_eq!(uhd_reply["releases"][1]["accepted"], false);

    // Unknown profileId is rejected with 422.
    let (status, bad) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases?profileId=99999"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    assert_eq!(bad["errors"][0]["field"], "profileId");

    // A movie_profile belonging to a different movie is also rejected.
    let other_movie = db::movies::create(
        &fx.app.state.db,
        db::movies::NewMovie {
            metadata: db::movies::Metadata {
                tmdb_id: 999,
                imdb_id: None,
                title: "Other".into(),
                original_title: None,
                sort_title: "other".into(),
                year: 2020,
                overview: None,
                runtime: Some(100),
                status: "released".into(),
                in_cinemas: None,
                digital_release: None,
                physical_release: None,
                poster_path: None,
            },
            root_folder_id: fx.root_id as i32,
            folder_name: "Other (2020)".into(),
            monitored: true,
            minimum_availability: "released".into(),
            collection_id: None,
        },
        &[fx.hd_profile_id as i32],
        &[],
    )
    .await
    .unwrap();
    let foreign_mp = other_movie.profiles[0].id;
    let (status, bad) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases?profileId={foreign_mp}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    assert_eq!(bad["errors"][0]["field"], "profileId");
}

#[tokio::test]
async fn search_cache_stores_releases_and_expires_entries_past_ttl() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::with_cache(
        &tmdb,
        SearchCache::with_config(Duration::from_millis(60), 16),
    )
    .await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();

    let server = MockServer::start().await;
    mount_torznab(
        &server,
        "key",
        torznab_feed(&[MockItem {
            title: "The.Matrix.1999.1080p.BluRay.x264-CACHE",
            guid: "cache-1",
            size: 8000 * MIB,
            seeders: 12,
            peers: 14,
            download_url: Some("http://idx.test/dl/cache-1"),
            magnet_url: Some("magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            info_hash: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            pub_date: None,
        }]),
    )
    .await;
    fx.add_indexer(json!({
        "name": "Idx",
        "url": server.uri(),
        "apiKey": "key",
    }))
    .await;

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let release_id = reply["releases"][0]["id"].as_str().unwrap();

    let cached = fx
        .app
        .state
        .search_cache
        .get(release_id)
        .expect("entry present before TTL");
    assert_eq!(cached.title, "The.Matrix.1999.1080p.BluRay.x264-CACHE");
    assert_eq!(
        cached.download_url.as_deref(),
        Some("http://idx.test/dl/cache-1")
    );
    assert_eq!(
        cached.info_hash.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );

    tokio::time::sleep(Duration::from_millis(90)).await;
    assert!(
        fx.app.state.search_cache.get(release_id).is_none(),
        "entry expired past TTL"
    );
}

#[tokio::test]
async fn requires_auth_and_search_run_permission_and_handles_missing_movie() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();
    let url = format!("/api/v1/movies/{movie_id}/releases");

    // 401 without authentication.
    let (status, _, reply) = fx.app.send(Method::GET, &url, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{reply}");
    assert_eq!(reply["code"], "unauthorized");

    // 403 without search:run.
    let reader = fx.app.member("reader@example.com", &["library:read"]).await;
    let (status, _, reply) = fx.app.send(Method::GET, &url, Some(&reader), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
    assert_eq!(reply["requiredPermission"], "search:run");

    // 200 with search:run (and empty results when no indexers exist).
    let searcher = fx.app.member("searcher@example.com", &["search:run"]).await;
    let (status, _, reply) = fx.app.send(Method::GET, &url, Some(&searcher), None).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["releases"], json!([]));
    assert_eq!(reply["indexerErrors"], json!([]));

    // 404 for unknown movie.
    let (status, _, reply) = fx
        .app
        .send(
            Method::GET,
            "/api/v1/movies/99999/releases",
            Some(&searcher),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
}

const NZBGEEK_CAPS: &str = include_str!("../../../fixtures/newznab/nzbgeek_caps.xml");

#[tokio::test]
async fn searches_and_ranks_newznab_and_torznab_indexers_together() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;
    let movie = fx.add_matrix(&[fx.hd_profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();

    let torrent_server = MockServer::start().await;
    let usenet_server = MockServer::start().await;

    mount_torznab(
        &torrent_server,
        "tor-key",
        torznab_feed(&[MockItem {
            title: "The.Matrix.1999.1080p.WEB-DL.x264-TOR",
            guid: "tor-webdl",
            size: 6000 * MIB,
            seeders: 100,
            peers: 120,
            download_url: Some("http://tor.test/dl/webdl"),
            magnet_url: None,
            info_hash: None,
            pub_date: None,
        }]),
    )
    .await;

    let nzb_feed = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/">
  <channel>
    <item>
      <title>The.Matrix.1999.1080p.BluRay.x264-NZB</title>
      <guid>nzb-bluray</guid>
      <pubDate>Sun, 22 Nov 2026 18:30:00 +0000</pubDate>
      <enclosure url="https://nzb.test/getnzb/bluray.nzb" length="{size}" type="application/x-nzb" />
      <newznab:attr name="size" value="{size}" />
      <newznab:attr name="category" value="2040" />
      <newznab:attr name="usenetdate" value="Sat, 21 Nov 2026 12:30:00 +0000" />
    </item>
  </channel>
</rss>"#,
        size = 10_000 * MIB
    );
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .and(query_param("apikey", "nzb-key"))
        .respond_with(xml(NZBGEEK_CAPS))
        .mount(&usenet_server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("imdbid", "0133093"))
        .and(query_param("apikey", "nzb-key"))
        .respond_with(xml(&nzb_feed))
        .mount(&usenet_server)
        .await;

    fx.add_indexer(json!({
        "name": "TorrentIdx",
        "kind": "torznab",
        "url": torrent_server.uri(),
        "apiKey": "tor-key",
        "priority": 10,
        "enabled": true,
    }))
    .await;
    fx.add_indexer(json!({
        "name": "UsenetIdx",
        "kind": "newznab",
        "url": usenet_server.uri(),
        "apiKey": "nzb-key",
        "priority": 25,
        "enabled": true,
    }))
    .await;

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["indexerErrors"], json!([]));

    let releases = reply["releases"].as_array().unwrap();
    assert_eq!(releases.len(), 2);
    assert_eq!(releases[0]["guid"], "nzb-bluray");
    assert_eq!(releases[0]["protocol"], "usenet");
    assert_eq!(releases[0]["publishedAt"], "2026-11-21T12:30:00.000Z");
    assert!(releases[0]["seeders"].is_null());
    assert!(releases[0]["leechers"].is_null());
    assert_eq!(releases[1]["guid"], "tor-webdl");
    assert_eq!(releases[1]["protocol"], "torrent");

    let cached = fx
        .app
        .state
        .search_cache
        .get(releases[0]["id"].as_str().unwrap())
        .expect("cached nzb release");
    assert_eq!(cached.protocol, "usenet");
    assert_eq!(
        cached.download_url.as_deref(),
        Some("https://nzb.test/getnzb/bluray.nzb")
    );
    assert!(cached.magnet_url.is_none());
    assert!(cached.info_hash.is_none());
}

#[tokio::test]
async fn custom_format_scores_rank_reject_and_enforce_cutoff_in_release_search() {
    let tmdb = MockServer::start().await;
    let fx = Fixture::new(&tmdb).await;

    let now = db::now();
    let hdr = custom_formats::ActiveModel {
        name: Set("HDR".into()),
        include_when_renaming: Set(false),
        specifications: Set(json!([{
            "name": "HDR",
            "implementation": "ReleaseTitleSpecification",
            "negate": false,
            "required": true,
            "fields": [{ "name": "value", "value": "\\bHDR\\b" }]
        }])),
        trash_id: Set(None),
        trash_synced: Set(false),
        created_at: Set(now.clone()),
        updated_at: Set(now.clone()),
        ..Default::default()
    }
    .insert(&fx.app.state.db)
    .await
    .unwrap();
    let atmos = custom_formats::ActiveModel {
        name: Set("Atmos".into()),
        include_when_renaming: Set(false),
        specifications: Set(json!([{
            "name": "Atmos",
            "implementation": "ReleaseTitleSpecification",
            "negate": false,
            "required": true,
            "fields": [{ "name": "value", "value": "\\bAtmos\\b" }]
        }])),
        trash_id: Set(None),
        trash_synced: Set(false),
        created_at: Set(now.clone()),
        updated_at: Set(now.clone()),
        ..Default::default()
    }
    .insert(&fx.app.state.db)
    .await
    .unwrap();

    // Configure profile: minFormatScore=100, cutoffFormatScore=500, minUpgradeFormatScore=100.
    // HDR = 500, Atmos = 200.
    let (status, profile) = fx
        .app
        .owner(
            Method::POST,
            "/api/v1/quality-profiles",
            Some(json!({
                "name": "CF Scored HD",
                "upgradeAllowed": true,
                "cutoff": 7,
                "minFormatScore": 100,
                "cutoffFormatScore": 500,
                "minUpgradeFormatScore": 100,
                "formatScores": [
                    { "customFormatId": hdr.id, "score": 500 },
                    { "customFormatId": atmos.id, "score": 200 }
                ],
                "items": [
                    { "type": "quality", "qualityId": 3, "allowed": true },
                    { "type": "quality", "qualityId": 7, "allowed": true }
                ]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{profile}");
    let profile_id = profile["id"].as_i64().unwrap();

    let movie = fx.add_matrix(&[profile_id]).await;
    let movie_id = movie["id"].as_i64().unwrap();
    let movie_profile_id = movie["profiles"][0]["id"].as_i64().unwrap();

    let idx_server = MockServer::start().await;
    mount_torznab(
        &idx_server,
        "cf-key",
        torznab_feed(&[
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.x264-LOW",
                guid: "cf-low",
                size: 8000 * MIB,
                seeders: 500,
                peers: 550,
                download_url: Some("http://cf.test/low"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.Atmos.x264-MID",
                guid: "cf-mid",
                size: 8000 * MIB,
                seeders: 200,
                peers: 220,
                download_url: Some("http://cf.test/mid"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
            MockItem {
                title: "The.Matrix.1999.1080p.BluRay.HDR.Atmos.x264-HIGH",
                guid: "cf-high",
                size: 8000 * MIB,
                seeders: 10,
                peers: 15,
                download_url: Some("http://cf.test/high"),
                magnet_url: None,
                info_hash: None,
                pub_date: None,
            },
        ]),
    )
    .await;
    fx.add_indexer(json!({
        "name": "CFIdx",
        "url": idx_server.uri(),
        "apiKey": "cf-key",
    }))
    .await;

    // 1. Without an existing file: HIGH (700) beats MID (200); LOW (0) is rejected for minFormatScore.
    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let releases = reply["releases"].as_array().unwrap();
    assert_eq!(releases.len(), 3);
    assert_eq!(releases[0]["guid"], "cf-high");
    assert_eq!(releases[0]["accepted"], true);
    assert_eq!(releases[0]["score"]["customFormatScore"], 700);
    assert_eq!(releases[1]["guid"], "cf-mid");
    assert_eq!(releases[1]["accepted"], true);
    assert_eq!(releases[1]["score"]["customFormatScore"], 200);
    assert_eq!(releases[2]["guid"], "cf-low");
    assert_eq!(releases[2]["accepted"], false);
    assert_eq!(releases[2]["score"]["customFormatScore"], 0);
    assert_eq!(releases[2]["rejections"][0]["code"], "min_format_score");

    // Verify CachedRelease carries the computed custom format score for downloads.
    let cached_high = fx
        .app
        .state
        .search_cache
        .get(releases[0]["id"].as_str().unwrap())
        .expect("cached high");
    assert_eq!(cached_high.custom_format_score, 700);

    // 2. Attach an existing Bluray-1080p file with HDR (score 500 == cutoffFormatScore).
    // Even HIGH (score 700) must NOT upgrade it because cutoffFormatScore is met.
    movie_files::ActiveModel {
        movie_id: Set(movie_id as i32),
        relative_path: Set("The.Matrix.1999.1080p.BluRay.HDR.x264-EXIST.mkv".into()),
        size: Set((8000 * MIB) as i64),
        quality_id: Set(7),
        scene_name: Set(Some("The.Matrix.1999.1080p.BluRay.HDR.x264-EXIST".into())),
        movie_profile_id: Set(Some(movie_profile_id as i32)),
        added_at: Set(now),
        ..Default::default()
    }
    .insert(&fx.app.state.db)
    .await
    .unwrap();

    let (status, reply) = fx
        .app
        .owner(
            Method::GET,
            &format!("/api/v1/movies/{movie_id}/releases"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let releases = reply["releases"].as_array().unwrap();
    let high = releases.iter().find(|r| r["guid"] == "cf-high").unwrap();
    assert_eq!(high["accepted"], false);
    assert_eq!(high["rejections"][0]["code"], "cutoff_met");
}
