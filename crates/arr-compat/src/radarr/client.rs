//! Read-only Radarr v3 HTTP client for instance import
//! (`docs/08-integrations.md`, "Radarr as import source (M6)", and
//! `docs/14-compatibility.md`, section C).
//!
//! Every request sends HTTP `GET` with `X-Api-Key` through [`OutboundClient`],
//! enforcing the outbound URL and DNS check. The client exposes no write
//! methods.

use std::fmt;

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::de::DeserializeOwned;
use unseenarr_core::http::{OutboundClient, UrlError, check_url};
use url::Url;

use super::dto::{
    CollectionResource, CustomFormatResource, DownloadClientResource, IndexerResource,
    MovieResource, QualityDefinitionResource, QualityProfileResource, RootFolderResource,
    SystemResource, TagResource,
};

const AGENT: &str = concat!("Unseenarr/", env!("CARGO_PKG_VERSION"));
const API_KEY_HEADER: &str = "X-Api-Key";

/// Why a read-only Radarr v3 API call failed.
#[derive(Debug, PartialEq, Eq)]
pub enum RadarrError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// Radarr refused the API key (HTTP 401 or 403).
    Unauthorized,
    /// The endpoint was not found (HTTP 404).
    NotFound(String),
    /// Radarr answered with an unexpected HTTP status.
    Status { status: u16, detail: String },
    /// Radarr answered with a body that did not match the expected JSON shape.
    InvalidResponse(String),
    /// Radarr could not be reached or the transfer failed.
    Unavailable(String),
}

impl fmt::Display for RadarrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "{error}"),
            Self::Unauthorized => f.write_str("Radarr refused the API key"),
            Self::NotFound(endpoint) => {
                write!(f, "Radarr endpoint `{endpoint}` was not found (HTTP 404)")
            }
            Self::Status { status, detail } if detail.is_empty() => {
                write!(f, "Radarr answered HTTP {status}")
            }
            Self::Status { status, detail } => {
                write!(f, "Radarr answered HTTP {status}: {detail}")
            }
            Self::InvalidResponse(reason) => write!(f, "unexpected Radarr answer: {reason}"),
            Self::Unavailable(reason) => write!(f, "Radarr is not available: {reason}"),
        }
    }
}

impl std::error::Error for RadarrError {}

impl From<UrlError> for RadarrError {
    fn from(error: UrlError) -> Self {
        Self::Url(error)
    }
}

/// Read-only client for a running Radarr v3 instance.
///
/// Accepts a base URL with or without a URL base, trailing slash, or `/api/v3`
/// suffix (for example `http://radarr:7878`, `http://radarr:7878/radarr/`, or
/// `http://radarr:7878/radarr/api/v3`).
#[derive(Clone)]
pub struct RadarrClient {
    http: OutboundClient,
    base_url: String,
    api_key: String,
}

// No derive: the API key must not reach a log line.
impl fmt::Debug for RadarrClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RadarrClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// Normalises a user-entered Radarr URL to `<origin>[<urlBase>]/api/v3/<endpoint>`
/// after running the outbound URL check.
fn endpoint_url(base_url: &str, endpoint: &str) -> Result<String, UrlError> {
    let mut url = Url::parse(base_url.trim()).map_err(|_| UrlError::Invalid)?;
    check_url(&url)?;
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/');
    let lower = path.to_ascii_lowercase();
    let api_base = match lower.strip_suffix("/api/v3") {
        Some(prefix) => format!("{}/api/v3", &path[..prefix.len()]),
        None => format!("{path}/api/v3"),
    };
    url.set_path(&format!("{api_base}/{}", endpoint.trim_start_matches('/')));
    Ok(url.into())
}

/// The first words of an error response body, collapsed to a single line.
fn snippet(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

fn unavailable(error: reqwest::Error) -> RadarrError {
    RadarrError::Unavailable(error.without_url().to_string())
}

impl RadarrClient {
    /// Creates a read-only client targeting `base_url` with `api_key`.
    pub fn new(http: OutboundClient, base_url: &str, api_key: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim().to_owned(),
            api_key: api_key.to_owned(),
        }
    }

    async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T, RadarrError> {
        let url = endpoint_url(&self.base_url, endpoint)?;
        let response = self
            .http
            .get(&url)?
            .header(API_KEY_HEADER, &self.api_key)
            .header(USER_AGENT, AGENT)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(unavailable)?;

        match response.status() {
            status if status.is_success() => {
                let body = response.bytes().await.map_err(unavailable)?;
                serde_json::from_slice(&body)
                    .map_err(|error| RadarrError::InvalidResponse(format!("{endpoint}: {error}")))
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(RadarrError::Unauthorized),
            StatusCode::NOT_FOUND => Err(RadarrError::NotFound(endpoint.to_owned())),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(RadarrError::Status {
                    status: status.as_u16(),
                    detail: snippet(&body),
                })
            }
        }
    }

    /// `GET /api/v3/system/status`
    pub async fn system_status(&self) -> Result<SystemResource, RadarrError> {
        self.get("system/status").await
    }

    /// `GET /api/v3/movie`
    pub async fn movies(&self) -> Result<Vec<MovieResource>, RadarrError> {
        self.get("movie").await
    }

    /// `GET /api/v3/rootfolder`
    pub async fn root_folders(&self) -> Result<Vec<RootFolderResource>, RadarrError> {
        self.get("rootfolder").await
    }

    /// `GET /api/v3/qualityprofile`
    pub async fn quality_profiles(&self) -> Result<Vec<QualityProfileResource>, RadarrError> {
        self.get("qualityprofile").await
    }

    /// `GET /api/v3/tag`
    pub async fn tags(&self) -> Result<Vec<TagResource>, RadarrError> {
        self.get("tag").await
    }

    /// `GET /api/v3/indexer`
    pub async fn indexers(&self) -> Result<Vec<IndexerResource>, RadarrError> {
        self.get("indexer").await
    }

    /// `GET /api/v3/downloadclient`
    pub async fn download_clients(&self) -> Result<Vec<DownloadClientResource>, RadarrError> {
        self.get("downloadclient").await
    }

    /// `GET /api/v3/customformat`
    pub async fn custom_formats(&self) -> Result<Vec<CustomFormatResource>, RadarrError> {
        self.get("customformat").await
    }

    /// `GET /api/v3/qualitydefinition`
    pub async fn quality_definitions(&self) -> Result<Vec<QualityDefinitionResource>, RadarrError> {
        self.get("qualitydefinition").await
    }

    /// `GET /api/v3/collection`
    pub async fn collections(&self) -> Result<Vec<CollectionResource>, RadarrError> {
        self.get("collection").await
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::radarr::dto::{
        AuthenticationType, DatabaseType, DownloadProtocol, Modifier, MovieStatusType,
        PrivacyLevel, QualitySource, RuntimeMode, UpdateMechanism,
    };

    const SYSTEM_STATUS: &str = include_str!("../../../../fixtures/radarr/system_status.json");
    const MOVIE: &str = include_str!("../../../../fixtures/radarr/movie.json");
    const ROOTFOLDER: &str = include_str!("../../../../fixtures/radarr/rootfolder.json");
    const QUALITYPROFILE: &str = include_str!("../../../../fixtures/radarr/qualityprofile.json");
    const TAG: &str = include_str!("../../../../fixtures/radarr/tag.json");
    const INDEXER: &str = include_str!("../../../../fixtures/radarr/indexer.json");
    const DOWNLOADCLIENT: &str = include_str!("../../../../fixtures/radarr/downloadclient.json");
    const CUSTOMFORMAT: &str = include_str!("../../../../fixtures/radarr/customformat.json");
    const QUALITYDEFINITION: &str =
        include_str!("../../../../fixtures/radarr/qualitydefinition.json");
    const COLLECTION: &str = include_str!("../../../../fixtures/radarr/collection.json");

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn json_ok(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
    }

    fn client(base_url: &str) -> RadarrClient {
        RadarrClient::new(OutboundClient::new().unwrap(), base_url, KEY)
    }

    async fn mount_get(server: &MockServer, url_path: &str, body: &str) {
        Mock::given(method("GET"))
            .and(path(url_path))
            .and(header("X-Api-Key", KEY))
            .and(header("User-Agent", AGENT))
            .and(header("Accept", "application/json"))
            .respond_with(json_ok(body))
            .expect(1)
            .mount(server)
            .await;
    }

    #[test]
    fn normalises_base_urls_with_or_without_url_base_and_api_prefix() {
        for (input, expected) in [
            (
                "http://radarr:7878",
                "http://radarr:7878/api/v3/system/status",
            ),
            (
                "http://radarr:7878/",
                "http://radarr:7878/api/v3/system/status",
            ),
            (
                "http://radarr:7878///",
                "http://radarr:7878/api/v3/system/status",
            ),
            (
                "http://radarr:7878/api/v3",
                "http://radarr:7878/api/v3/system/status",
            ),
            (
                "http://radarr:7878/api/v3/",
                "http://radarr:7878/api/v3/system/status",
            ),
            (
                "http://radarr:7878/radarr",
                "http://radarr:7878/radarr/api/v3/system/status",
            ),
            (
                "http://radarr:7878/radarr/",
                "http://radarr:7878/radarr/api/v3/system/status",
            ),
            (
                "http://radarr:7878/radarr/api/v3",
                "http://radarr:7878/radarr/api/v3/system/status",
            ),
            (
                "http://radarr:7878/radarr/api/v3/",
                "http://radarr:7878/radarr/api/v3/system/status",
            ),
            (
                "  https://media.example/Radarr/API/V3/?ignored=1#frag  ",
                "https://media.example/Radarr/api/v3/system/status",
            ),
            (
                "http://radarr:7878/myapi/v3",
                "http://radarr:7878/myapi/v3/api/v3/system/status",
            ),
        ] {
            assert_eq!(
                endpoint_url(input, "system/status").as_deref(),
                Ok(expected),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn system_status_parses_fixture_and_handles_url_base_variants() {
        let server = MockServer::start().await;
        mount_get(&server, "/api/v3/system/status", SYSTEM_STATUS).await;

        let status = client(&server.uri()).system_status().await.unwrap();
        assert_eq!(status.app_name, "Radarr");
        assert_eq!(status.version, "7.0.3.10918");
        assert_eq!(status.url_base, "/radarr");
        assert_eq!(status.mode, RuntimeMode::Console);
        assert_eq!(status.database_type, DatabaseType::SqLite);
        assert_eq!(status.authentication, AuthenticationType::Forms);
        assert_eq!(status.package_update_mechanism, UpdateMechanism::Docker);
        // `packageUpdateMechanismMessage` is explicit `null` in the fixture.
        assert_eq!(status.package_update_mechanism_message, "");

        let with_prefix = MockServer::start().await;
        for base in [
            format!("{}/radarr", with_prefix.uri()),
            format!("{}/radarr/", with_prefix.uri()),
            format!("{}/radarr/api/v3", with_prefix.uri()),
            format!("{}/radarr/api/v3/", with_prefix.uri()),
        ] {
            Mock::given(method("GET"))
                .and(path("/radarr/api/v3/system/status"))
                .and(header("X-Api-Key", KEY))
                .respond_with(json_ok(SYSTEM_STATUS))
                .up_to_n_times(1)
                .mount(&with_prefix)
                .await;
            let answer = client(&base).system_status().await.unwrap();
            assert_eq!(answer.app_name, "Radarr", "{base}");
        }
    }

    #[tokio::test]
    async fn movies_parses_fixture_with_and_without_movie_file() {
        let server = MockServer::start().await;
        mount_get(&server, "/api/v3/movie", MOVIE).await;

        let movies = client(&server.uri()).movies().await.unwrap();
        assert_eq!(movies.len(), 2);

        let matrix = &movies[0];
        assert_eq!(matrix.id, 1);
        assert_eq!(matrix.tmdb_id, 603);
        assert_eq!(matrix.imdb_id, "tt0133093");
        assert_eq!(matrix.title, "The Matrix");
        assert_eq!(matrix.year, 1999);
        assert_eq!(matrix.status, MovieStatusType::Released);
        assert_eq!(matrix.tags, [1, 2]);
        assert_eq!(matrix.collection.as_ref().unwrap().tmdb_id, 2344);
        let file = matrix.movie_file.as_ref().unwrap();
        assert_eq!(file.id, 10);
        assert_eq!(file.quality.quality.id, 7);
        assert_eq!(file.quality.quality.source, QualitySource::Bluray);
        assert_eq!(file.release_group, "SPARKS");
        assert_eq!(file.media_info.video_codec, "x264");

        let dune = &movies[1];
        assert_eq!(dune.tmdb_id, 693134);
        assert_eq!(dune.has_file, Some(false));
        assert!(dune.movie_file.is_none());
        assert!(dune.collection.is_none());
        // `website` is explicit `null` in the fixture.
        assert_eq!(dune.website, "");
    }

    #[tokio::test]
    async fn root_folders_quality_profiles_and_tags_parse_fixtures() {
        let server = MockServer::start().await;
        mount_get(&server, "/api/v3/rootfolder", ROOTFOLDER).await;
        mount_get(&server, "/api/v3/qualityprofile", QUALITYPROFILE).await;
        mount_get(&server, "/api/v3/tag", TAG).await;

        let client = client(&format!("{}/", server.uri()));

        let folders = client.root_folders().await.unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].path, "/movies");
        assert!(folders[0].accessible);
        assert_eq!(folders[0].unmapped_folders.len(), 1);
        assert_eq!(
            folders[0].unmapped_folders[0].name,
            "Unmatched Movie (2020)"
        );

        let profiles = client.quality_profiles().await.unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "HD-1080p");
        assert!(profiles[0].upgrade_allowed);
        assert_eq!(profiles[0].cutoff, 1000);
        assert_eq!(profiles[0].language.id, 1);
        assert!(profiles[0].items[1].quality.is_none());
        assert_eq!(profiles[0].items[1].name, "WEB 1080p");
        assert_eq!(profiles[0].items[1].items.len(), 2);
        assert_eq!(profiles[0].format_items.len(), 2);
        assert_eq!(profiles[0].format_items[0].score, -10000);
        assert_eq!(profiles[1].language.id, -2);

        let tags = client.tags().await.unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].label, "4k");
        assert_eq!(tags[1].label, "family");
    }

    #[tokio::test]
    async fn indexers_and_download_clients_parse_fixtures_and_fields() {
        let server = MockServer::start().await;
        mount_get(&server, "/api/v3/indexer", INDEXER).await;
        mount_get(&server, "/api/v3/downloadclient", DOWNLOADCLIENT).await;

        let client = client(&format!("{}/api/v3", server.uri()));

        let indexers = client.indexers().await.unwrap();
        assert_eq!(indexers.len(), 2);
        assert_eq!(indexers[0].implementation, "Torznab");
        assert_eq!(indexers[0].protocol, DownloadProtocol::Torrent);
        assert_eq!(indexers[0].tags, [1]);
        assert_eq!(
            indexers[0]
                .field("baseUrl")
                .and_then(|field| field.value.as_ref())
                .and_then(serde_json::Value::as_str),
            Some("http://prowlarr:9696/1/")
        );
        let api_key = indexers[0].field("apiKey").unwrap();
        assert_eq!(api_key.privacy, PrivacyLevel::ApiKey);
        assert_eq!(
            api_key.value.as_ref().and_then(serde_json::Value::as_str),
            Some("********")
        );
        assert!(indexers[0].field("missing").is_none());
        assert_eq!(indexers[1].implementation, "Newznab");
        assert_eq!(indexers[1].protocol, DownloadProtocol::Usenet);

        let clients = client.download_clients().await.unwrap();
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].implementation, "QBittorrent");
        assert!(clients[0].enable);
        assert_eq!(clients[0].protocol, DownloadProtocol::Torrent);
        assert_eq!(
            clients[0]
                .field("movieCategory")
                .and_then(|field| field.value.as_ref())
                .and_then(serde_json::Value::as_str),
            Some("radarr")
        );
        assert_eq!(
            clients[0]
                .field("recentMoviePriority")
                .unwrap()
                .select_options
                .len(),
            2
        );
        assert!(clients[0].field("missing").is_none());
        assert_eq!(clients[1].implementation, "Sabnzbd");
        assert!(!clients[1].enable);
    }

    #[tokio::test]
    async fn custom_formats_quality_definitions_and_collections_parse_fixtures() {
        let server = MockServer::start().await;
        mount_get(&server, "/api/v3/customformat", CUSTOMFORMAT).await;
        mount_get(&server, "/api/v3/qualitydefinition", QUALITYDEFINITION).await;
        mount_get(&server, "/api/v3/collection", COLLECTION).await;

        let client = client(&server.uri());

        let formats = client.custom_formats().await.unwrap();
        assert_eq!(formats.len(), 2);
        assert_eq!(formats[0].name, "x265 (HD)");
        assert_eq!(formats[0].specifications.len(), 2);
        assert_eq!(
            formats[0].specifications[0].implementation,
            "ReleaseTitleSpecification"
        );
        assert_eq!(formats[0].specifications[0].fields.len(), 1);
        assert!(formats[0].specifications[1].negate);
        assert_eq!(
            formats[0].specifications[1].fields[0].select_options.len(),
            2
        );

        let defs = client.quality_definitions().await.unwrap();
        assert_eq!(defs.len(), 30);
        assert_eq!(defs[0].title, "Unknown");
        assert_eq!(defs[0].max_size, Some(100.0));
        // From Bluray-1080p upward Radarr omits `maxSize` and `preferredSize`.
        let bluray_1080p = defs.iter().find(|d| d.quality.id == 7).unwrap();
        assert_eq!(bluray_1080p.min_size, Some(0.0));
        assert_eq!(bluray_1080p.max_size, None);
        assert_eq!(bluray_1080p.preferred_size, None);
        let remux_2160p = defs.iter().find(|d| d.quality.id == 31).unwrap();
        assert_eq!(remux_2160p.quality.modifier, Modifier::Remux);

        let collections = client.collections().await.unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].tmdb_id, 2344);
        assert_eq!(collections[0].title, "The Matrix Collection");
        assert!(collections[0].monitored);
        assert_eq!(collections[0].missing_movies, 1);
        assert_eq!(collections[0].movies.len(), 2);
        assert!(collections[0].movies[0].is_existing);
        assert!(!collections[0].movies[1].is_existing);
    }

    #[tokio::test]
    async fn status_codes_and_invalid_json_map_to_typed_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v3/system/status"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v3/tag"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v3/movie"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v3/rootfolder"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal\n  error"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v3/indexer"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v3/collection"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>login</html>"))
            .mount(&server)
            .await;

        let client = client(&server.uri());
        assert_eq!(
            client.system_status().await.unwrap_err(),
            RadarrError::Unauthorized
        );
        assert_eq!(client.tags().await.unwrap_err(), RadarrError::Unauthorized);
        assert_eq!(
            client.movies().await.unwrap_err(),
            RadarrError::NotFound("movie".to_owned())
        );
        assert_eq!(
            client.root_folders().await.unwrap_err(),
            RadarrError::Status {
                status: 500,
                detail: "Internal error".to_owned(),
            }
        );
        assert_eq!(
            client.indexers().await.unwrap_err(),
            RadarrError::Status {
                status: 502,
                detail: String::new(),
            }
        );
        assert!(matches!(
            client.collections().await.unwrap_err(),
            RadarrError::InvalidResponse(reason) if reason.starts_with("collection:")
        ));
    }

    #[tokio::test]
    async fn bad_urls_are_refused_before_any_request_is_sent() {
        assert_eq!(
            client("not a url").system_status().await.unwrap_err(),
            RadarrError::Url(UrlError::Invalid)
        );
        assert_eq!(
            client("ftp://radarr:7878")
                .system_status()
                .await
                .unwrap_err(),
            RadarrError::Url(UrlError::Scheme)
        );
        assert_eq!(
            client("http://169.254.169.254:7878")
                .system_status()
                .await
                .unwrap_err(),
            RadarrError::Url(UrlError::Address)
        );
    }

    #[tokio::test]
    async fn unreachable_server_and_blocked_redirect_fail_without_leaking_key() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_port = listener.local_addr().unwrap().port();
        drop(listener);

        let error = client(&format!("http://127.0.0.1:{closed_port}"))
            .system_status()
            .await
            .unwrap_err();
        let RadarrError::Unavailable(reason) = &error else {
            panic!("expected Unavailable, got {error:?}");
        };
        assert!(!reason.contains(KEY), "{reason}");

        // OutboundClient refuses to follow a redirect to a blocked host.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v3/system/status"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://169.254.169.254/latest/meta-data/"),
            )
            .mount(&server)
            .await;
        assert_eq!(
            client(&server.uri()).system_status().await.unwrap_err(),
            RadarrError::Status {
                status: 302,
                detail: String::new(),
            }
        );
    }

    #[test]
    fn debug_hides_api_key_and_errors_format_clearly() {
        let client = client("http://radarr:7878");
        let debug = format!("{client:?}");
        assert!(debug.contains("http://radarr:7878"), "{debug}");
        assert!(!debug.contains(KEY), "{debug}");

        assert_eq!(
            RadarrError::Url(UrlError::Address).to_string(),
            "this address is not allowed"
        );
        assert_eq!(
            RadarrError::Unauthorized.to_string(),
            "Radarr refused the API key"
        );
        assert_eq!(
            RadarrError::NotFound("movie".into()).to_string(),
            "Radarr endpoint `movie` was not found (HTTP 404)"
        );
        assert_eq!(
            RadarrError::Status {
                status: 500,
                detail: String::new(),
            }
            .to_string(),
            "Radarr answered HTTP 500"
        );
        assert_eq!(
            RadarrError::Status {
                status: 500,
                detail: "boom".into(),
            }
            .to_string(),
            "Radarr answered HTTP 500: boom"
        );
        assert_eq!(
            RadarrError::InvalidResponse("bad json".into()).to_string(),
            "unexpected Radarr answer: bad json"
        );
        assert_eq!(
            RadarrError::Unavailable("connection refused".into()).to_string(),
            "Radarr is not available: connection refused"
        );
    }
}
