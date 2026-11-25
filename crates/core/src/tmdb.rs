//! The TMDB implementation of [`MetadataProvider`] (docs/08-integrations.md).
//!
//! TMDB is never proxied. The base URL is a parameter so tests can point the
//! client at a mock server.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::http::OutboundClient;
use crate::metadata::{
    Collection, CollectionRef, MetadataError, MetadataProvider, MovieDetails, MovieSummary,
    ReleaseDate, ReleaseKind,
};

pub const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org";
pub const DEFAULT_LANGUAGE: &str = "en-US";

/// Retries after a 429, so a request is sent at most `MAX_RETRIES + 1` times.
const MAX_RETRIES: u32 = 3;
/// The longest wait for one retry, well under the 30 s client timeout.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// True for `en` and `en-US`: what TMDB accepts as `language`.
pub fn is_valid_language(language: &str) -> bool {
    let (code, region) = match language.split_once('-') {
        Some((code, region)) => (code, Some(region)),
        None => (language, None),
    };
    code.len() == 2
        && code.bytes().all(|b| b.is_ascii_lowercase())
        && region.is_none_or(|r| r.len() == 2 && r.bytes().all(|b| b.is_ascii_uppercase()))
}

/// How long to wait before retry number `retries` (from 0): the seconds in
/// `Retry-After`, else 1 s, 2 s, 4 s. Never more than [`MAX_WAIT`].
pub(crate) fn retry_wait(retry_after: Option<&str>, retries: u32) -> Duration {
    let wait = retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(Duration::from_secs(1 << retries), Duration::from_secs);
    wait.min(MAX_WAIT)
}

pub struct TmdbClient {
    http: OutboundClient,
    base_url: String,
    api_key: String,
    language: String,
}

// No derive: the key must not reach a log line.
impl fmt::Debug for TmdbClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TmdbClient")
            .field("base_url", &self.base_url)
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl TmdbClient {
    /// `api_key` is a v3 key (sent as `api_key`) or a v4 read access token,
    /// which is a JWT (sent as a bearer token).
    pub fn new(http: OutboundClient, base_url: &str, api_key: &str, language: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.to_owned(),
            language: language.to_owned(),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, MetadataError> {
        let url = format!("{}{path}", self.base_url);
        let mut retries = 0;
        loop {
            let mut request = self
                .http
                .request(Method::GET, &url)
                .map_err(|error| MetadataError::Unavailable(error.to_string()))?
                .query(&[("language", self.language.as_str())])
                .query(params);
            request = if self.api_key.starts_with("eyJ") {
                request.bearer_auth(&self.api_key)
            } else {
                request.query(&[("api_key", self.api_key.as_str())])
            };
            // A reqwest error prints the URL, which holds the key.
            let response = request
                .send()
                .await
                .map_err(|error| MetadataError::Unavailable(error.without_url().to_string()))?;
            match response.status() {
                StatusCode::TOO_MANY_REQUESTS => {
                    if retries == MAX_RETRIES {
                        return Err(MetadataError::RateLimited);
                    }
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok());
                    tokio::time::sleep(retry_wait(retry_after, retries)).await;
                    retries += 1;
                }
                StatusCode::UNAUTHORIZED => return Err(MetadataError::Unauthorized),
                StatusCode::NOT_FOUND => return Err(MetadataError::NotFound),
                status if !status.is_success() => {
                    return Err(MetadataError::Unavailable(format!(
                        "TMDB answered {status}"
                    )));
                }
                _ => {
                    let body = response.bytes().await.map_err(|error| {
                        MetadataError::Unavailable(error.without_url().to_string())
                    })?;
                    return serde_json::from_slice(&body).map_err(|error| {
                        MetadataError::Unavailable(format!("unexpected TMDB answer: {error}"))
                    });
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct RawMovie {
    id: i32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    original_title: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
}

impl From<RawMovie> for MovieSummary {
    fn from(raw: RawMovie) -> Self {
        Self {
            tmdb_id: raw.id,
            title: raw.title,
            original_title: raw.original_title,
            overview: raw.overview.unwrap_or_default(),
            // TMDB sends "" for an unknown date.
            release_date: raw.release_date.filter(|date| !date.is_empty()),
            poster_path: raw.poster_path,
        }
    }
}

#[derive(Deserialize)]
struct Results<T> {
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

#[derive(Deserialize)]
struct FindResponse {
    #[serde(default)]
    movie_results: Vec<RawMovie>,
}

#[derive(Deserialize)]
struct RawDetails {
    #[serde(flatten)]
    movie: RawMovie,
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    genres: Vec<RawNamed>,
    #[serde(default)]
    belongs_to_collection: Option<RawCollectionRef>,
    #[serde(default)]
    external_ids: Option<RawExternalIds>,
    #[serde(default)]
    release_dates: Option<Results<RawCountry>>,
}

#[derive(Deserialize)]
struct RawNamed {
    name: String,
}

#[derive(Deserialize)]
struct RawCollectionRef {
    id: i32,
    name: String,
}

#[derive(Deserialize)]
struct RawExternalIds {
    #[serde(default)]
    imdb_id: Option<String>,
}

#[derive(Deserialize)]
struct RawCountry {
    iso_3166_1: String,
    #[serde(default)]
    release_dates: Vec<RawReleaseDate>,
}

#[derive(Deserialize)]
struct RawReleaseDate {
    #[serde(default)]
    certification: String,
    release_date: String,
    #[serde(rename = "type")]
    kind: i64,
}

impl From<RawDetails> for MovieDetails {
    fn from(raw: RawDetails) -> Self {
        let non_empty = |id: Option<String>| id.filter(|id| !id.is_empty());
        let imdb_id = non_empty(raw.external_ids.and_then(|ids| ids.imdb_id))
            .or_else(|| non_empty(raw.imdb_id));
        let release_dates = raw
            .release_dates
            .map(|dates| dates.results)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|country| {
                let code = country.iso_3166_1;
                country.release_dates.into_iter().filter_map(move |date| {
                    Some(ReleaseDate {
                        country: code.clone(),
                        kind: ReleaseKind::from_code(date.kind)?,
                        date: date.release_date,
                        certification: Some(date.certification).filter(|c| !c.is_empty()),
                    })
                })
            })
            .collect();
        Self {
            summary: raw.movie.into(),
            imdb_id,
            runtime_minutes: raw.runtime.filter(|minutes| *minutes > 0),
            status: raw.status,
            genres: raw.genres.into_iter().map(|genre| genre.name).collect(),
            collection: raw.belongs_to_collection.map(|c| CollectionRef {
                tmdb_id: c.id,
                name: c.name,
            }),
            release_dates,
        }
    }
}

#[derive(Deserialize)]
struct RawCollection {
    id: i32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    parts: Vec<RawMovie>,
}

#[async_trait]
impl MetadataProvider for TmdbClient {
    async fn search(&self, term: &str) -> Result<Vec<MovieSummary>, MetadataError> {
        let page: Results<RawMovie> = self
            .get(
                "/3/search/movie",
                &[("query", term), ("include_adult", "false")],
            )
            .await?;
        Ok(page.results.into_iter().map(Into::into).collect())
    }

    async fn find_by_imdb_id(&self, imdb_id: &str) -> Result<Vec<MovieSummary>, MetadataError> {
        let found: FindResponse = self
            .get(
                &format!("/3/find/{imdb_id}"),
                &[("external_source", "imdb_id")],
            )
            .await?;
        Ok(found.movie_results.into_iter().map(Into::into).collect())
    }

    async fn details(&self, tmdb_id: i32) -> Result<MovieDetails, MetadataError> {
        let raw: RawDetails = self
            .get(
                &format!("/3/movie/{tmdb_id}"),
                &[("append_to_response", "external_ids,release_dates")],
            )
            .await?;
        Ok(raw.into())
    }

    async fn collection(&self, tmdb_id: i32) -> Result<Collection, MetadataError> {
        let raw: RawCollection = self.get(&format!("/3/collection/{tmdb_id}"), &[]).await?;
        Ok(Collection {
            tmdb_id: raw.id,
            name: raw.name,
            overview: raw.overview.unwrap_or_default(),
            poster_path: raw.poster_path,
            movies: raw.parts.into_iter().map(Into::into).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const SEARCH: &str = include_str!("../../../fixtures/tmdb/search_movie.json");
    const MOVIE: &str = include_str!("../../../fixtures/tmdb/movie_603.json");
    const FIND: &str = include_str!("../../../fixtures/tmdb/find_imdb.json");
    const FIND_EMPTY: &str = include_str!("../../../fixtures/tmdb/find_empty.json");
    const COLLECTION: &str = include_str!("../../../fixtures/tmdb/collection_2344.json");
    const INVALID_KEY: &str = include_str!("../../../fixtures/tmdb/error_invalid_key.json");
    const NOT_FOUND: &str = include_str!("../../../fixtures/tmdb/error_not_found.json");

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
    }

    fn client(server: &MockServer) -> TmdbClient {
        TmdbClient::new(OutboundClient::new().unwrap(), &server.uri(), KEY, "fr-FR")
    }

    #[tokio::test]
    async fn search_sends_the_query_key_and_language() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/3/search/movie"))
            .and(query_param("query", "the matrix"))
            .and(query_param("api_key", KEY))
            .and(query_param("language", "fr-FR"))
            .and(query_param("include_adult", "false"))
            .respond_with(json(SEARCH))
            .expect(1)
            .mount(&server)
            .await;

        let found = client(&server).search("the matrix").await.unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].tmdb_id, 603);
        assert_eq!(found[0].title, "The Matrix");
        assert_eq!(found[0].year(), Some(1999));
        assert_eq!(
            found[0].poster_path.as_deref(),
            Some("/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg")
        );
        // An unknown date is absent, not an empty string.
        assert_eq!(found[2].release_date, None);
        assert_eq!(found[2].overview, "");
    }

    #[tokio::test]
    async fn details_carry_external_ids_collection_and_release_dates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/3/movie/603"))
            .and(query_param(
                "append_to_response",
                "external_ids,release_dates",
            ))
            .respond_with(json(MOVIE))
            .expect(1)
            .mount(&server)
            .await;

        let details = client(&server).details(603).await.unwrap();
        assert_eq!(details.summary.tmdb_id, 603);
        assert_eq!(details.imdb_id.as_deref(), Some("tt0133093"));
        assert_eq!(details.runtime_minutes, Some(136));
        assert_eq!(details.status, "Released");
        assert_eq!(details.genres, ["Action", "Science Fiction"]);
        assert_eq!(
            details.collection,
            Some(CollectionRef {
                tmdb_id: 2344,
                name: "The Matrix Collection".into()
            })
        );
        // The unknown type 99 is skipped; an empty certification is absent.
        assert_eq!(
            details.release_dates,
            [
                ReleaseDate {
                    country: "US".into(),
                    kind: ReleaseKind::Theatrical,
                    date: "1999-03-31T00:00:00.000Z".into(),
                    certification: Some("R".into()),
                },
                ReleaseDate {
                    country: "US".into(),
                    kind: ReleaseKind::Physical,
                    date: "1999-09-21T00:00:00.000Z".into(),
                    certification: Some("R".into()),
                },
                ReleaseDate {
                    country: "FR".into(),
                    kind: ReleaseKind::Theatrical,
                    date: "1999-06-23T00:00:00.000Z".into(),
                    certification: None,
                },
                ReleaseDate {
                    country: "FR".into(),
                    kind: ReleaseKind::Digital,
                    date: "2009-01-01T00:00:00.000Z".into(),
                    certification: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn details_without_the_appended_parts_still_parse() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/3/movie/7"))
            .respond_with(json(
                r#"{"id": 7, "title": "Bare", "imdb_id": "tt0000007", "runtime": 0}"#,
            ))
            .mount(&server)
            .await;

        let details = client(&server).details(7).await.unwrap();
        assert_eq!(details.imdb_id.as_deref(), Some("tt0000007"));
        assert_eq!(details.runtime_minutes, None);
        assert_eq!(details.collection, None);
        assert!(details.release_dates.is_empty());
    }

    #[tokio::test]
    async fn find_by_imdb_id_names_the_external_source() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/3/find/tt0133093"))
            .and(query_param("external_source", "imdb_id"))
            .respond_with(json(FIND))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/3/find/tt0000001"))
            .respond_with(json(FIND_EMPTY))
            .mount(&server)
            .await;

        let client = client(&server);
        let found = client.find_by_imdb_id("tt0133093").await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tmdb_id, 603);
        assert!(
            client
                .find_by_imdb_id("tt0000001")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn collection_lists_its_movies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/3/collection/2344"))
            .respond_with(json(COLLECTION))
            .expect(1)
            .mount(&server)
            .await;

        let collection = client(&server).collection(2344).await.unwrap();
        assert_eq!(collection.tmdb_id, 2344);
        assert_eq!(collection.name, "The Matrix Collection");
        assert_eq!(collection.movies.len(), 2);
        assert_eq!(collection.movies[1].tmdb_id, 604);
    }

    #[tokio::test]
    async fn a_read_access_token_goes_in_the_authorization_header() {
        let server = MockServer::start().await;
        let token = "eyJhbGciOiJIUzI1NiJ9.payload.signature";
        Mock::given(method("GET"))
            .and(path("/3/search/movie"))
            .and(header("authorization", format!("Bearer {token}").as_str()))
            .respond_with(json(SEARCH))
            .expect(1)
            .mount(&server)
            .await;

        let client = TmdbClient::new(
            OutboundClient::new().unwrap(),
            &server.uri(),
            token,
            "en-US",
        );
        client.search("x").await.unwrap();
        // The token is not repeated in the URL.
        let requests = server.received_requests().await.unwrap();
        assert!(!requests[0].url.as_str().contains("eyJ"));
    }

    #[tokio::test]
    async fn status_codes_map_to_errors() {
        let server = MockServer::start().await;
        for (id, response) in [
            (
                1,
                ResponseTemplate::new(401)
                    .set_body_raw(INVALID_KEY.as_bytes().to_vec(), "application/json"),
            ),
            (
                2,
                ResponseTemplate::new(404)
                    .set_body_raw(NOT_FOUND.as_bytes().to_vec(), "application/json"),
            ),
            (3, ResponseTemplate::new(500)),
            (4, json("not json")),
        ] {
            Mock::given(method("GET"))
                .and(path(format!("/3/movie/{id}")))
                .respond_with(response)
                .mount(&server)
                .await;
        }

        let client = client(&server);
        assert_eq!(
            client.details(1).await.unwrap_err(),
            MetadataError::Unauthorized
        );
        assert_eq!(
            client.details(2).await.unwrap_err(),
            MetadataError::NotFound
        );
        assert_eq!(
            client.details(3).await.unwrap_err(),
            MetadataError::Unavailable("TMDB answered 500 Internal Server Error".into())
        );
        assert!(matches!(
            client.details(4).await.unwrap_err(),
            MetadataError::Unavailable(why) if why.starts_with("unexpected TMDB answer")
        ));
    }

    #[tokio::test]
    async fn a_429_is_retried_after_the_wait() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/3/search/movie"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/3/search/movie"))
            .respond_with(json(SEARCH))
            .expect(1)
            .mount(&server)
            .await;

        let found = client(&server).search("matrix").await.unwrap();
        assert_eq!(found.len(), 3);
    }

    #[tokio::test]
    async fn a_429_that_does_not_end_is_reported_after_the_retries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .expect(u64::from(MAX_RETRIES) + 1)
            .mount(&server)
            .await;

        assert_eq!(
            client(&server).search("matrix").await.unwrap_err(),
            MetadataError::RateLimited
        );
    }

    #[test]
    fn the_wait_follows_retry_after_and_is_capped() {
        let secs = Duration::from_secs;
        assert_eq!(retry_wait(Some("3"), 0), secs(3));
        assert_eq!(retry_wait(Some(" 0 "), 2), secs(0));
        assert_eq!(retry_wait(Some("3600"), 0), MAX_WAIT);
        // No usable header: 1 s, 2 s, 4 s.
        assert_eq!(retry_wait(None, 0), secs(1));
        assert_eq!(
            retry_wait(Some("Wed, 21 Oct 2026 07:28:00 GMT"), 1),
            secs(2)
        );
        assert_eq!(retry_wait(None, 2), secs(4));
    }

    #[tokio::test]
    async fn a_blocked_address_is_refused_and_the_key_stays_out_of_the_error() {
        let client = TmdbClient::new(
            OutboundClient::new().unwrap(),
            "http://169.254.169.254",
            KEY,
            "en-US",
        );
        let MetadataError::Unavailable(why) = client.search("x").await.unwrap_err() else {
            panic!("expected Unavailable");
        };
        assert!(!why.contains(KEY));

        // A refused connection: the URL, with its key, is not in the message.
        let closed = free_port().await;
        let client = TmdbClient::new(
            OutboundClient::new().unwrap(),
            &format!("http://127.0.0.1:{closed}"),
            KEY,
            "en-US",
        );
        let MetadataError::Unavailable(why) = client.search("x").await.unwrap_err() else {
            panic!("expected Unavailable");
        };
        assert!(!why.contains(KEY), "{why}");
    }

    /// A port with nothing listening on it.
    async fn free_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    #[test]
    fn languages() {
        for valid in ["en", "en-US", "fr-FR", "pt-BR"] {
            assert!(is_valid_language(valid), "{valid}");
        }
        for invalid in [
            "", "e", "eng", "EN", "en-us", "en_US", "en-USA", "en-", "-US",
        ] {
            assert!(!is_valid_language(invalid), "{invalid}");
        }
    }

    #[test]
    fn debug_hides_the_key() {
        let client = TmdbClient::new(
            OutboundClient::new().unwrap(),
            DEFAULT_BASE_URL,
            KEY,
            "en-US",
        );
        assert!(!format!("{client:?}").contains(KEY));
    }
}
