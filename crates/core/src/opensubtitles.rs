//! The OpenSubtitles implementation of [`SubtitleProvider`]
//! (docs/08-integrations.md): REST API v1, `POST /login`, `GET /subtitles`
//! and `POST /download`.
//!
//! The user's own API key, username and password are parameters; Unseen ships
//! no shared key. The base URL is a parameter so tests can point the client at
//! a mock server. OpenSubtitles is never proxied.
//!
//! A search uses no quota. Each download uses one unit; the answer says how
//! many are left and when they come back. The client keeps that answer, and
//! once no unit is left it refuses downloads without calling the provider
//! until the reset time. All calls go through one limiter with a minimum
//! delay between them.

use std::fmt;
use std::sync::{Mutex as StdMutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER, USER_AGENT};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::http::{OutboundClient, UrlError};
use crate::subtitle_provider::{
    DownloadedSubtitle, Quota, SubtitleCandidate, SubtitleError, SubtitleKind, SubtitleProvider,
    SubtitleQuery,
};
use crate::tmdb::retry_wait;

pub const DEFAULT_BASE_URL: &str = "https://api.opensubtitles.com/api/v1";

/// Retries after a 429, so a request is sent at most `MAX_RETRIES + 1` times.
const MAX_RETRIES: u32 = 3;
/// Between two calls to the API, unless the Owner sets another delay.
pub const DEFAULT_MIN_DELAY: Duration = Duration::from_secs(1);
/// How long downloads stop when the provider refuses one and gives no reset
/// time. Only used for that missing answer.
const UNKNOWN_RESET: Duration = Duration::from_secs(3600);
const AGENT: &str = concat!("Unseen/", env!("CARGO_PKG_VERSION"));

impl From<UrlError> for SubtitleError {
    fn from(error: UrlError) -> Self {
        Self::Unavailable(error.to_string())
    }
}

fn unavailable(error: reqwest::Error) -> SubtitleError {
    // `without_url`: the reason must not echo the address.
    SubtitleError::Unavailable(error.without_url().to_string())
}

fn unexpected(status: StatusCode) -> SubtitleError {
    SubtitleError::Unavailable(format!("OpenSubtitles answered {status}"))
}

/// The statuses that mean the same for every call.
fn failure(status: StatusCode) -> SubtitleError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => SubtitleError::Unauthorized,
        StatusCode::NOT_FOUND => SubtitleError::NotFound,
        StatusCode::TOO_MANY_REQUESTS => SubtitleError::RateLimited,
        status => unexpected(status),
    }
}

fn now() -> DateTime<Utc> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
    DateTime::from_timestamp(secs, 0).unwrap_or(DateTime::UNIX_EPOCH)
}

fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct OpenSubtitlesClient {
    http: OutboundClient,
    base_url: String,
    api_key: String,
    username: String,
    password: String,
    min_delay: Duration,
    last_call: Mutex<Option<Instant>>,
    token: Mutex<Option<String>>,
    quota: StdMutex<Option<Quota>>,
}

// No derive: the key and the password must not reach a log line.
impl fmt::Debug for OpenSubtitlesClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenSubtitlesClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl OpenSubtitlesClient {
    /// `base_url` is [`DEFAULT_BASE_URL`] outside tests.
    pub fn new(
        http: OutboundClient,
        base_url: &str,
        api_key: &str,
        username: &str,
        password: &str,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
            min_delay: DEFAULT_MIN_DELAY,
            last_call: Mutex::new(None),
            token: Mutex::new(None),
            quota: StdMutex::new(None),
        }
    }

    /// The minimum delay between two calls to the API.
    pub fn with_min_delay(mut self, delay: Duration) -> Self {
        self.min_delay = delay;
        self
    }

    /// Starts from the quota stored after the last run, so a restart does not
    /// call the provider while the quota is used up.
    pub fn with_quota(self, quota: Option<Quota>) -> Self {
        *lock(&self.quota) = quota;
        self
    }

    /// The quota from the provider's last answer, for the caller to store.
    pub fn quota(&self) -> Option<Quota> {
        *lock(&self.quota)
    }

    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, SubtitleError> {
        Ok(self
            .http
            .request(method, &format!("{}{path}", self.base_url))?
            .header("Api-Key", &self.api_key)
            .header(USER_AGENT, AGENT)
            .header(ACCEPT, "application/json"))
    }

    fn json_request(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<RequestBuilder, SubtitleError> {
        Ok(self
            .request(Method::POST, path)?
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string()))
    }

    /// Sends a request through the limiter. A 429 is retried after the wait
    /// that `Retry-After` gives; when it persists, the last 429 comes back for
    /// the caller to judge.
    async fn call(&self, request: RequestBuilder) -> Result<Response, SubtitleError> {
        let mut retries = 0;
        loop {
            {
                let mut last = self.last_call.lock().await;
                if let Some(wait) = last.and_then(|at| self.min_delay.checked_sub(at.elapsed())) {
                    tokio::time::sleep(wait).await;
                }
                *last = Some(Instant::now());
            }
            let attempt = request.try_clone().ok_or_else(|| {
                SubtitleError::Unavailable("the request cannot be sent twice".into())
            })?;
            let response = attempt.send().await.map_err(unavailable)?;
            if response.status() != StatusCode::TOO_MANY_REQUESTS || retries == MAX_RETRIES {
                return Ok(response);
            }
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok());
            tokio::time::sleep(retry_wait(retry_after, retries)).await;
            retries += 1;
        }
    }

    async fn login(&self) -> Result<String, SubtitleError> {
        if self.username.is_empty() || self.password.is_empty() {
            return Err(SubtitleError::Unauthorized);
        }
        let body = serde_json::json!({ "username": self.username, "password": self.password });
        let response = self.call(self.json_request("/login", &body)?).await?;
        if response.status() != StatusCode::OK {
            return Err(failure(response.status()));
        }
        let login: Login = parse(&response.bytes().await.map_err(unavailable)?)?;
        if login.token.is_empty() {
            return Err(SubtitleError::Unavailable(
                "the login answer holds no token".into(),
            ));
        }
        Ok(login.token)
    }

    /// The bearer token, from the login the first time.
    async fn token(&self) -> Result<String, SubtitleError> {
        let mut token = self.token.lock().await;
        if let Some(token) = &*token {
            return Ok(token.clone());
        }
        let fresh = self.login().await?;
        *token = Some(fresh.clone());
        Ok(fresh)
    }

    /// Refuses a download while the quota is used up and its reset time has
    /// not come.
    fn check_quota(&self) -> Result<(), SubtitleError> {
        let mut quota = lock(&self.quota);
        if let Some(Quota {
            remaining: 0,
            resets_at: Some(resets_at),
        }) = *quota
        {
            if resets_at > now() {
                return Err(SubtitleError::QuotaExhausted { resets_at });
            }
            *quota = None;
        }
        Ok(())
    }

    /// Keeps the quota of an answer. With none left and no reset time, the
    /// reset is a guess ([`UNKNOWN_RESET`]), so downloads still stop.
    fn record_quota(&self, answer: &DownloadAnswer) -> Option<Quota> {
        let remaining = u32::try_from(answer.remaining?.max(0)).unwrap_or(u32::MAX);
        let mut resets_at = answer
            .reset_time_utc
            .as_deref()
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.with_timezone(&Utc))
            .filter(|time| *time > now());
        if remaining == 0 && resets_at.is_none() {
            resets_at = Some(now() + UNKNOWN_RESET);
        }
        let quota = Quota {
            remaining,
            resets_at,
        };
        *lock(&self.quota) = Some(quota);
        Some(quota)
    }

    /// The error for a refused download; later downloads stop until the reset.
    fn exhausted(&self, body: &[u8]) -> SubtitleError {
        let answer: DownloadAnswer = parse(body).unwrap_or_default();
        let quota = self.record_quota(&DownloadAnswer {
            remaining: Some(0),
            ..answer
        });
        SubtitleError::QuotaExhausted {
            // `record_quota` sets a reset time when none is left.
            resets_at: quota.and_then(|quota| quota.resets_at).unwrap_or_else(now),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, SubtitleError> {
    serde_json::from_slice(body)
        .map_err(|error| SubtitleError::Unavailable(format!("unexpected answer: {error}")))
}

/// The `hearing_impaired` and `foreign_parts_only` parameters for the kinds
/// wanted, `None` where the provider's default (`include`) is right.
fn kind_filters(kinds: &[SubtitleKind]) -> (Option<&'static str>, Option<&'static str>) {
    let wants = |kind| kinds.is_empty() || kinds.contains(&kind);
    let (normal, hearing, forced) = (
        wants(SubtitleKind::Normal),
        wants(SubtitleKind::HearingImpaired),
        wants(SubtitleKind::Forced),
    );
    let filter = |wanted: bool, others: bool| match (wanted, others) {
        (true, true) => None,
        (true, false) => Some("only"),
        (false, _) => Some("exclude"),
    };
    (
        filter(hearing, normal || forced),
        filter(forced, normal || hearing),
    )
}

/// The search parameters, sorted by name as the provider asks for its cache.
fn search_params(query: &SubtitleQuery) -> Vec<(&'static str, String)> {
    let mut params = vec![("type", "movie".to_owned())];
    // An id is exact; the title is the fallback. Two ids would have to agree.
    let imdb_id = query.imdb_id.as_deref().and_then(|id| {
        id.trim()
            .trim_start_matches("tt")
            .parse::<u64>()
            .ok()
            .map(|number| number.to_string())
    });
    if let Some(tmdb_id) = query.tmdb_id {
        params.push(("tmdb_id", tmdb_id.to_string()));
    } else if let Some(imdb_id) = imdb_id {
        params.push(("imdb_id", imdb_id));
    } else if !query.title.trim().is_empty() {
        params.push(("query", query.title.trim().to_owned()));
        if let Some(year) = query.year {
            params.push(("year", year.to_string()));
        }
    }
    if let Some(hash) = query.hash.as_deref().filter(|hash| !hash.is_empty()) {
        params.push(("moviehash", hash.to_ascii_lowercase()));
    }
    if !query.languages.is_empty() {
        let mut languages = query.languages.clone();
        languages.sort();
        languages.dedup();
        params.push(("languages", languages.join(",")));
    }
    let (hearing_impaired, foreign_parts_only) = kind_filters(&query.kinds);
    if let Some(value) = hearing_impaired {
        params.push(("hearing_impaired", value.to_owned()));
    }
    if let Some(value) = foreign_parts_only {
        params.push(("foreign_parts_only", value.to_owned()));
    }
    params.sort();
    params
}

#[derive(Deserialize)]
struct Login {
    #[serde(default)]
    token: String,
}

#[derive(Deserialize)]
struct SearchAnswer {
    #[serde(default)]
    data: Vec<RawSubtitle>,
}

#[derive(Deserialize)]
struct RawSubtitle {
    attributes: RawAttributes,
}

// The provider sends `null` for some of these, hence the options.
#[derive(Deserialize)]
struct RawAttributes {
    language: Option<String>,
    download_count: Option<u64>,
    hearing_impaired: Option<bool>,
    ratings: Option<f64>,
    from_trusted: Option<bool>,
    foreign_parts_only: Option<bool>,
    ai_translated: Option<bool>,
    machine_translated: Option<bool>,
    release: Option<String>,
    moviehash_match: Option<bool>,
    #[serde(default)]
    files: Vec<RawFile>,
}

#[derive(Deserialize)]
struct RawFile {
    file_id: u64,
}

impl RawSubtitle {
    /// `None` for a subtitle that cannot be downloaded: no file, no language.
    fn into_candidate(self) -> Option<SubtitleCandidate> {
        let a = self.attributes;
        let kind = if a.foreign_parts_only == Some(true) {
            SubtitleKind::Forced
        } else if a.hearing_impaired == Some(true) {
            SubtitleKind::HearingImpaired
        } else {
            SubtitleKind::Normal
        };
        Some(SubtitleCandidate {
            id: a.files.first()?.file_id.to_string(),
            language: a.language.filter(|language| !language.is_empty())?,
            kind,
            release: a.release.unwrap_or_default(),
            hash_match: a.moviehash_match == Some(true),
            trusted: a.from_trusted == Some(true),
            download_count: a.download_count.unwrap_or_default(),
            rating: a.ratings.unwrap_or_default(),
            machine_translated: a.ai_translated == Some(true) || a.machine_translated == Some(true),
        })
    }
}

/// The answer to `POST /download`, and the body of a refusal, which carries
/// the same quota fields.
#[derive(Deserialize, Default)]
struct DownloadAnswer {
    link: Option<String>,
    file_name: Option<String>,
    remaining: Option<i64>,
    reset_time_utc: Option<String>,
}

/// `srt` from `Movie.srt`; `srt` too when the name has no usable extension.
fn extension(file_name: Option<&str>) -> String {
    file_name
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| {
            (1..=5).contains(&extension.len())
                && extension.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "srt".to_owned())
}

#[async_trait]
impl SubtitleProvider for OpenSubtitlesClient {
    async fn search(&self, query: &SubtitleQuery) -> Result<Vec<SubtitleCandidate>, SubtitleError> {
        let params = search_params(query);
        // Languages and kinds alone would list every subtitle there is.
        if !params
            .iter()
            .any(|(name, _)| matches!(*name, "tmdb_id" | "imdb_id" | "query" | "moviehash"))
        {
            return Ok(Vec::new());
        }
        let request = self.request(Method::GET, "/subtitles")?.query(&params);
        let response = self.call(request).await?;
        if response.status() != StatusCode::OK {
            return Err(failure(response.status()));
        }
        let answer: SearchAnswer = parse(&response.bytes().await.map_err(unavailable)?)?;
        Ok(answer
            .data
            .into_iter()
            .filter_map(RawSubtitle::into_candidate)
            .collect())
    }

    async fn download(&self, id: &str) -> Result<DownloadedSubtitle, SubtitleError> {
        self.check_quota()?;
        let file_id: u64 = id.trim().parse().map_err(|_| SubtitleError::NotFound)?;
        let body = serde_json::json!({ "file_id": file_id });
        let mut relogged = false;
        let (answer, link) = loop {
            let token = self.token().await?;
            let request = self.json_request("/download", &body)?.bearer_auth(&token);
            let response = self.call(request).await?;
            match response.status() {
                StatusCode::OK => {
                    let answer: DownloadAnswer =
                        parse(&response.bytes().await.map_err(unavailable)?)?;
                    let link = answer.link.clone().ok_or_else(|| {
                        SubtitleError::Unavailable("the download answer holds no link".into())
                    })?;
                    break (answer, link);
                }
                // The token lasts a day: log in once more, then give up.
                StatusCode::UNAUTHORIZED if !relogged => {
                    relogged = true;
                    *self.token.lock().await = None;
                }
                // Not acceptable: the download count is exceeded. A 429 that
                // the retries did not clear is treated the same way.
                StatusCode::NOT_ACCEPTABLE | StatusCode::TOO_MANY_REQUESTS => {
                    let body = response.bytes().await.map_err(unavailable)?;
                    return Err(self.exhausted(&body));
                }
                status => return Err(failure(status)),
            }
        };
        let quota = self.record_quota(&answer);
        // The file is on another host and needs neither key nor token.
        let file = self
            .http
            .get(&link)?
            .header(USER_AGENT, AGENT)
            .send()
            .await
            .map_err(unavailable)?;
        if !file.status().is_success() {
            return Err(SubtitleError::Unavailable(format!(
                "the subtitle file answered {}",
                file.status()
            )));
        }
        let bytes = file.bytes().await.map_err(unavailable)?.to_vec();
        if bytes.is_empty() {
            return Err(SubtitleError::Unavailable(
                "the subtitle file is empty".into(),
            ));
        }
        Ok(DownloadedSubtitle {
            bytes,
            extension: extension(answer.file_name.as_deref()),
            quota,
        })
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;

    const LOGIN: &str = include_str!("../../../fixtures/opensubtitles/login.json");
    const LOGIN_INVALID: &str = include_str!("../../../fixtures/opensubtitles/login_invalid.json");
    const INVALID_KEY: &str =
        include_str!("../../../fixtures/opensubtitles/error_invalid_key.json");
    const SEARCH: &str = include_str!("../../../fixtures/opensubtitles/search.json");
    const SEARCH_EMPTY: &str = include_str!("../../../fixtures/opensubtitles/search_empty.json");
    const DOWNLOAD: &str = include_str!("../../../fixtures/opensubtitles/download.json");
    const DOWNLOAD_LAST: &str = include_str!("../../../fixtures/opensubtitles/download_last.json");
    const EXHAUSTED: &str =
        include_str!("../../../fixtures/opensubtitles/download_quota_exhausted.json");
    const SUBTITLE: &[u8] = include_bytes!("../../../fixtures/opensubtitles/subtitle.srt");

    fn client(server: &MockServer) -> OpenSubtitlesClient {
        OpenSubtitlesClient::new(
            OutboundClient::new().unwrap(),
            &format!("{}/", server.uri()),
            "key-123",
            "neo",
            "pa&ss",
        )
        .with_min_delay(Duration::ZERO)
    }

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
    }

    fn reset_2099() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2099-01-02T07:09:51Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn mount_login(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/login"))
            .and(header("api-key", "key-123"))
            .and(body_json(
                serde_json::json!({ "username": "neo", "password": "pa&ss" }),
            ))
            .respond_with(json(LOGIN))
            .mount(server)
            .await;
    }

    /// The download endpoint answering `body` (`{{base}}` is the server) and
    /// the file it links to.
    async fn mount_download(server: &MockServer, body: &str) {
        Mock::given(method("POST"))
            .and(path("/download"))
            .and(header("api-key", "key-123"))
            .and(header("authorization", "Bearer eyJ.test-token.signature"))
            .and(body_json(serde_json::json!({ "file_id": 90001 })))
            .respond_with(json(&body.replace("{{base}}", &server.uri())))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/files/90001/The.Matrix.1999.1080p.BluRay.x264-GROUP.srt",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(SUBTITLE))
            .mount(server)
            .await;
    }

    fn query() -> SubtitleQuery {
        SubtitleQuery {
            tmdb_id: Some(603),
            imdb_id: Some("tt0133093".into()),
            title: "The Matrix".into(),
            year: Some(1999),
            languages: vec!["fr".into(), "en".into()],
            kinds: vec![],
            hash: Some("8E245D9679D31E12".into()),
            file_name: Some("The.Matrix.1999.1080p.BluRay.x264-GROUP.mkv".into()),
        }
    }

    #[tokio::test]
    async fn search_by_tmdb_id_and_hash_lists_the_candidates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .and(header("api-key", "key-123"))
            .and(query_param("tmdb_id", "603"))
            .and(query_param("moviehash", "8e245d9679d31e12"))
            .and(query_param("languages", "en,fr"))
            .and(query_param("type", "movie"))
            .respond_with(json(SEARCH))
            .expect(1)
            .mount(&server)
            .await;
        let candidates = client(&server).search(&query()).await.unwrap();

        // The fourth entry has no file to download.
        let ids: Vec<_> = candidates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["90001", "90002", "90003"]);
        assert_eq!(
            candidates[0],
            SubtitleCandidate {
                id: "90001".into(),
                language: "en".into(),
                kind: SubtitleKind::Normal,
                release: "The.Matrix.1999.1080p.BluRay.x264-GROUP".into(),
                hash_match: true,
                trusted: true,
                download_count: 120_450,
                rating: 8.5,
                machine_translated: false,
            }
        );
        assert_eq!(candidates[1].kind, SubtitleKind::HearingImpaired);
        assert!(!candidates[1].hash_match);
        assert_eq!(candidates[2].kind, SubtitleKind::Forced);
        assert!(candidates[2].machine_translated, "AI translated");
    }

    #[tokio::test]
    async fn search_uses_the_user_agent_and_needs_no_login() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(|request: &Request| {
                let agent = request.headers.get("user-agent").unwrap().to_str().unwrap();
                assert!(agent.starts_with("Unseen/"), "{agent}");
                assert!(!request.headers.contains_key("authorization"));
                json(SEARCH_EMPTY)
            })
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(json(LOGIN))
            .expect(0)
            .mount(&server)
            .await;
        assert_eq!(client(&server).search(&query()).await, Ok(vec![]));
    }

    #[tokio::test]
    async fn search_falls_back_to_the_imdb_id_then_the_title() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .and(query_param("imdb_id", "133093"))
            .respond_with(json(SEARCH_EMPTY))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .and(query_param("query", "The Matrix"))
            .and(query_param("year", "1999"))
            .respond_with(json(SEARCH_EMPTY))
            .expect(1)
            .mount(&server)
            .await;
        let client = client(&server);
        let mut by_imdb = query();
        by_imdb.tmdb_id = None;
        client.search(&by_imdb).await.unwrap();
        let mut by_title = by_imdb;
        by_title.imdb_id = None;
        client.search(&by_title).await.unwrap();

        // Nothing to look for: no call.
        let nothing = SubtitleQuery::default();
        assert_eq!(client.search(&nothing).await, Ok(vec![]));
    }

    #[test]
    fn kinds_become_the_two_filters() {
        use SubtitleKind::{Forced, HearingImpaired, Normal};
        assert_eq!(kind_filters(&[]), (None, None));
        assert_eq!(kind_filters(&[Normal]), (Some("exclude"), Some("exclude")));
        assert_eq!(
            kind_filters(&[HearingImpaired]),
            (Some("only"), Some("exclude"))
        );
        assert_eq!(kind_filters(&[Forced]), (Some("exclude"), Some("only")));
        assert_eq!(
            kind_filters(&[Normal, HearingImpaired]),
            (None, Some("exclude"))
        );
        assert_eq!(kind_filters(&[Normal, Forced]), (Some("exclude"), None));
    }

    #[tokio::test]
    async fn search_sends_the_kind_filters_in_alphabetical_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .and(query_param("hearing_impaired", "exclude"))
            .and(query_param("foreign_parts_only", "only"))
            .respond_with(|request: &Request| {
                let names: Vec<_> = request
                    .url
                    .query_pairs()
                    .map(|(k, _)| k.into_owned())
                    .collect();
                let mut sorted = names.clone();
                sorted.sort();
                assert_eq!(names, sorted);
                json(SEARCH_EMPTY)
            })
            .expect(1)
            .mount(&server)
            .await;
        let mut forced = query();
        forced.kinds = vec![SubtitleKind::Forced];
        client(&server).search(&forced).await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_api_key_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(ResponseTemplate::new(403).set_body_raw(INVALID_KEY, "application/json"))
            .mount(&server)
            .await;
        assert_eq!(
            client(&server).search(&query()).await,
            Err(SubtitleError::Unauthorized)
        );
    }

    #[tokio::test]
    async fn a_download_logs_in_once_and_returns_the_file_and_the_quota() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(json(LOGIN))
            .expect(1)
            .mount(&server)
            .await;
        mount_download(&server, DOWNLOAD).await;
        let client = client(&server);

        let subtitle = client.download("90001").await.unwrap();
        assert_eq!(subtitle.bytes, SUBTITLE);
        assert_eq!(subtitle.extension, "srt");
        let quota = Quota {
            remaining: 17,
            resets_at: Some(reset_2099()),
        };
        assert_eq!(subtitle.quota, Some(quota));
        assert_eq!(client.quota(), Some(quota));

        // The token is kept: no second login (the mock expects one).
        client.download("90001").await.unwrap();
    }

    #[tokio::test]
    async fn the_file_request_carries_neither_key_nor_token() {
        let server = MockServer::start().await;
        mount_login(&server).await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .respond_with(json(&DOWNLOAD.replace("{{base}}", &server.uri())))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/files/90001/The.Matrix.1999.1080p.BluRay.x264-GROUP.srt",
            ))
            .respond_with(|request: &Request| {
                assert!(!request.headers.contains_key("api-key"));
                assert!(!request.headers.contains_key("authorization"));
                ResponseTemplate::new(200).set_body_bytes(SUBTITLE)
            })
            .expect(1)
            .mount(&server)
            .await;
        client(&server).download("90001").await.unwrap();
    }

    #[tokio::test]
    async fn a_wrong_password_is_unauthorized_and_nothing_is_downloaded() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(401).set_body_raw(LOGIN_INVALID, "application/json"),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        assert_eq!(
            client(&server).download("90001").await,
            Err(SubtitleError::Unauthorized)
        );
    }

    #[tokio::test]
    async fn a_missing_login_is_unauthorized_without_a_call() {
        let server = MockServer::start().await;
        let client =
            OpenSubtitlesClient::new(OutboundClient::new().unwrap(), &server.uri(), "key", "", "");
        assert_eq!(
            client.download("90001").await,
            Err(SubtitleError::Unauthorized)
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_expired_token_is_replaced_once() {
        let server = MockServer::start().await;
        let login = |token: &str| json(&LOGIN.replace("eyJ.test-token.signature", token));
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(login("old-token"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(login("eyJ.test-token.signature"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .and(header("authorization", "Bearer old-token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        mount_download(&server, DOWNLOAD).await;
        assert_eq!(
            client(&server).download("90001").await.unwrap().bytes,
            SUBTITLE
        );
    }

    #[tokio::test]
    async fn an_unknown_subtitle_is_not_found() {
        let server = MockServer::start().await;
        mount_login(&server).await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let client = client(&server);
        assert_eq!(client.download("90001").await, Err(SubtitleError::NotFound));
        assert_eq!(
            client.download("not-a-number").await,
            Err(SubtitleError::NotFound)
        );
    }

    #[tokio::test]
    async fn the_last_unit_is_used_and_then_downloads_stop_until_the_reset() {
        let server = MockServer::start().await;
        mount_login(&server).await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .respond_with(json(&DOWNLOAD_LAST.replace("{{base}}", &server.uri())))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/files/90001/The.Matrix.1999.1080p.BluRay.x264-GROUP.srt",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(SUBTITLE))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(json(SEARCH_EMPTY))
            .expect(1)
            .mount(&server)
            .await;
        let client = client(&server);

        let last = client.download("90001").await.unwrap();
        assert_eq!(last.quota.map(|quota| quota.remaining), Some(0));
        // The mock expects one download call: this one is not sent.
        assert_eq!(
            client.download("90001").await,
            Err(SubtitleError::QuotaExhausted {
                resets_at: reset_2099()
            })
        );
        // A search uses no quota and still goes out.
        client.search(&query()).await.unwrap();
    }

    #[tokio::test]
    async fn a_406_stops_downloads_until_the_reset_time_it_gives() {
        let server = MockServer::start().await;
        mount_login(&server).await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .respond_with(ResponseTemplate::new(406).set_body_raw(EXHAUSTED, "application/json"))
            .expect(1)
            .mount(&server)
            .await;
        let client = client(&server);
        let exhausted = Err(SubtitleError::QuotaExhausted {
            resets_at: reset_2099(),
        });
        assert_eq!(client.download("90001").await, exhausted);
        assert_eq!(client.download("90001").await, exhausted);
        assert_eq!(
            client.quota(),
            Some(Quota {
                remaining: 0,
                resets_at: Some(reset_2099())
            })
        );
    }

    #[tokio::test]
    async fn downloads_resume_once_the_reset_time_has_passed() {
        let server = MockServer::start().await;
        mount_login(&server).await;
        mount_download(&server, DOWNLOAD).await;
        let stored = Quota {
            remaining: 0,
            resets_at: DateTime::from_timestamp(1_700_000_000, 0),
        };
        let client = client(&server).with_quota(Some(stored));
        assert_eq!(client.download("90001").await.unwrap().bytes, SUBTITLE);
        assert_eq!(client.quota().map(|quota| quota.remaining), Some(17));
    }

    #[tokio::test]
    async fn a_stored_quota_stops_downloads_after_a_restart() {
        let server = MockServer::start().await;
        let stored = Quota {
            remaining: 0,
            resets_at: Some(reset_2099()),
        };
        let client = client(&server).with_quota(Some(stored));
        assert_eq!(
            client.download("90001").await,
            Err(SubtitleError::QuotaExhausted {
                resets_at: reset_2099()
            })
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_429_is_retried_after_the_wait_it_gives() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(json(SEARCH))
            .mount(&server)
            .await;
        assert_eq!(client(&server).search(&query()).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_search_that_stays_limited_gives_up() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .expect(u64::from(MAX_RETRIES) + 1)
            .mount(&server)
            .await;
        assert_eq!(
            client(&server).search(&query()).await,
            Err(SubtitleError::RateLimited)
        );
    }

    #[tokio::test]
    async fn a_download_that_stays_limited_is_treated_as_an_exhausted_quota() {
        let server = MockServer::start().await;
        mount_login(&server).await;
        Mock::given(method("POST"))
            .and(path("/download"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .expect(u64::from(MAX_RETRIES) + 1)
            .mount(&server)
            .await;
        let client = client(&server);
        let Err(SubtitleError::QuotaExhausted { resets_at }) = client.download("90001").await
        else {
            panic!("expected an exhausted quota");
        };
        assert!(resets_at > now());
        // No new call (the mock counts the first four only).
        assert_eq!(
            client.download("90001").await,
            Err(SubtitleError::QuotaExhausted { resets_at })
        );
    }

    #[tokio::test]
    async fn calls_are_spaced_by_the_minimum_delay() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(json(SEARCH_EMPTY))
            .mount(&server)
            .await;
        let client = client(&server).with_min_delay(Duration::from_millis(200));
        let started = Instant::now();
        client.search(&query()).await.unwrap();
        client.search(&query()).await.unwrap();
        client.search(&query()).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(400));
    }

    #[tokio::test]
    async fn a_server_error_is_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(matches!(
            client(&server).search(&query()).await,
            Err(SubtitleError::Unavailable(_))
        ));
        let other = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/subtitles"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&other)
            .await;
        assert!(matches!(
            client(&other).search(&query()).await,
            Err(SubtitleError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn a_refused_address_is_not_called() {
        let client = OpenSubtitlesClient::new(
            OutboundClient::new().unwrap(),
            "http://169.254.169.254",
            "key",
            "u",
            "p",
        );
        assert!(matches!(
            client.search(&query()).await,
            Err(SubtitleError::Unavailable(_))
        ));
    }

    #[test]
    fn the_extension_comes_from_the_file_name() {
        assert_eq!(extension(Some("Movie.2020.SRT")), "srt");
        assert_eq!(extension(Some("Movie.ass")), "ass");
        assert_eq!(extension(Some("Movie")), "srt");
        assert_eq!(extension(Some("Movie.../../x")), "srt");
        assert_eq!(extension(None), "srt");
    }

    #[test]
    fn debug_hides_the_key_and_the_password() {
        let client = OpenSubtitlesClient::new(
            OutboundClient::new().unwrap(),
            "http://o",
            "key-123",
            "neo",
            "hunter2",
        );
        let shown = format!("{client:?}");
        assert!(!shown.contains("key-123") && !shown.contains("hunter2") && !shown.contains("neo"));
    }
}
