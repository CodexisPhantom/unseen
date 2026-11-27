//! The qBittorrent WebUI API v2 client (docs/08-integrations.md): the
//! session, the version call that the test button uses, and the torrent
//! calls (add, list, properties, delete).
//!
//! The torrent calls follow Radarr's category convention. Each call names the
//! category of the download client, and a torrent in another category (or in
//! none) is never listed and never touched, even when its info hash is given.
//! The client keeps the save path: Unseenarr reads `content_path` from the
//! torrent and never sets a path.
//!
//! Two ways to authenticate. With a username and a password the client logs
//! in and keeps the session cookie. qBittorrent answers a login with HTTP 200
//! and the body `Ok.` or `Fails.`, so the status code alone does not tell a
//! wrong password from a good one. With an API key (qBittorrent 5.2 or later)
//! every request carries `Authorization: Bearer <key>` and there is no login:
//! a key cannot call the auth endpoints.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};

use reqwest::header::{AUTHORIZATION, COOKIE, REFERER, SET_COOKIE};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::http::{OutboundClient, UrlError};

#[derive(Debug, PartialEq, Eq)]
pub enum QbError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// qBittorrent answered, and refused the username or password.
    Credentials,
    /// qBittorrent answered, and refused the API key.
    ApiKey,
    /// qBittorrent banned this address after failed logins (HTTP 403).
    Banned,
    /// No answer, or an answer that is not from the WebUI API.
    Unavailable(String),
}

impl fmt::Display for QbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "{error}"),
            Self::Credentials => f.write_str("qBittorrent refused the username or password"),
            Self::ApiKey => f.write_str("qBittorrent refused the API key"),
            Self::Banned => f.write_str("qBittorrent banned this address after failed logins"),
            Self::Unavailable(reason) => write!(f, "qBittorrent is not available: {reason}"),
        }
    }
}

impl std::error::Error for QbError {}

impl From<UrlError> for QbError {
    fn from(error: UrlError) -> Self {
        Self::Url(error)
    }
}

/// How the client proves who it is.
pub enum QbAuth {
    /// Log in with these and use the session cookie. Both may be empty when
    /// qBittorrent bypasses authentication for the address of this server.
    Password { username: String, password: String },
    /// An API key of qBittorrent 5.2 or later (`qbt_...`).
    ApiKey(String),
}

pub struct QbClient {
    http: OutboundClient,
    base_url: String,
    auth: QbAuth,
    /// The session cookie of the last login (`SID=...`, empty when
    /// qBittorrent let this address in without one). Shared by every call, so
    /// a poll and an add do not each log in.
    session: Mutex<Option<String>>,
}

/// A torrent of the category, as `torrents/info` lists it. A field the client
/// does not send keeps its default.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Torrent {
    /// The info hash, lower case. The identity of the torrent.
    pub hash: String,
    pub name: String,
    pub category: String,
    /// `downloading`, `stalledUP`, `pausedDL`, `error`... as qBittorrent names it.
    pub state: String,
    /// 0 to 1.
    pub progress: f64,
    pub size: i64,
    pub amount_left: i64,
    /// Bytes per second.
    pub dlspeed: i64,
    /// Seconds; 8640000 means unknown.
    pub eta: i64,
    pub ratio: f64,
    /// Seconds.
    pub seeding_time: i64,
    /// Unix time; -1 when not completed.
    pub completion_on: i64,
    pub added_on: i64,
    pub save_path: String,
    /// The file or folder of the torrent. Unseenarr imports from here, as the
    /// client reports it.
    pub content_path: String,
}

/// The `torrents/properties` answer, the fields Unseenarr reads.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct TorrentProperties {
    pub save_path: String,
    /// Unix time; -1 when not completed.
    pub completion_date: i64,
    pub addition_date: i64,
    pub total_size: i64,
    pub total_downloaded: i64,
    pub share_ratio: f64,
    /// Seconds.
    pub seeding_time: i64,
}

// No derive: the password and the key must not reach a log line.
impl fmt::Debug for QbClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QbClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// The step of the test, so an error says where it happened. The body is the
/// first words of a text answer, which tells a qBittorrent message from a
/// proxy page.
async fn unexpected(step: &str, response: reqwest::Response) -> QbError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let body: String = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect();
    QbError::Unavailable(if body.is_empty() {
        format!("{step} answered HTTP {status}")
    } else {
        format!("{step} answered HTTP {status}: {body}")
    })
}

fn unavailable(error: reqwest::Error) -> QbError {
    // `without_url`: the reason must not echo the address with credentials.
    QbError::Unavailable(error.without_url().to_string())
}

impl QbClient {
    /// `base_url` is the WebUI address, for example `http://qbittorrent:8080`.
    pub fn new(http: OutboundClient, base_url: &str, auth: QbAuth) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth,
            session: Mutex::new(None),
        }
    }

    /// Logs in and returns the session cookie (`SID=...`).
    async fn login(&self, username: &str, password: &str) -> Result<String, QbError> {
        let response = self
            .http
            .request(
                Method::POST,
                &format!("{}/api/v2/auth/login", self.base_url),
            )?
            // qBittorrent rejects a request whose Referer is not its own host.
            .header(REFERER, &self.base_url)
            .form(&[("username", username), ("password", password)])
            .send()
            .await
            .map_err(unavailable)?;
        let status = response.status();
        match status {
            // Older versions answer 200 with a body; newer ones may answer
            // 204 without one.
            StatusCode::OK | StatusCode::NO_CONTENT => {}
            StatusCode::UNAUTHORIZED => return Err(QbError::Credentials),
            StatusCode::FORBIDDEN => return Err(QbError::Banned),
            _ => return Err(unexpected("the login", response).await),
        }
        let cookie = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|cookie| cookie.starts_with("SID="))
            .and_then(|cookie| cookie.split(';').next())
            .map(str::to_owned);
        let body = response.text().await.map_err(unavailable)?;
        if status == StatusCode::NO_CONTENT {
            return Ok(cookie.unwrap_or_default());
        }
        match (body.trim(), cookie) {
            ("Fails.", _) => Err(QbError::Credentials),
            // With "bypass authentication for localhost" there is no cookie.
            ("Ok.", cookie) => Ok(cookie.unwrap_or_default()),
            _ => Err(QbError::Unavailable(
                "the address does not answer like the qBittorrent WebUI".to_owned(),
            )),
        }
    }

    /// Authenticates and returns the qBittorrent version, for example
    /// `v4.6.2`.
    pub async fn test(&self) -> Result<String, QbError> {
        let response = self
            .call(
                "the version call",
                Method::GET,
                "/api/v2/app/version",
                |r| r,
            )
            .await?;
        Ok(response
            .text()
            .await
            .map_err(unavailable)?
            .trim()
            .to_owned())
    }

    /// The session cookie to use. `stale` is the cookie that qBittorrent just
    /// refused: when the stored one is another, a concurrent call has already
    /// logged in again and its cookie is used.
    async fn cookie(
        &self,
        username: &str,
        password: &str,
        stale: Option<&str>,
    ) -> Result<String, QbError> {
        let mut session = self.session.lock().await;
        if let Some(cookie) = session.as_deref()
            && Some(cookie) != stale
        {
            return Ok(cookie.to_owned());
        }
        let cookie = self.login(username, password).await?;
        *session = Some(cookie.clone());
        Ok(cookie)
    }

    /// Sends one request with the credentials and returns the answer when it
    /// is a success. `build` adds the query or the form. An expired session
    /// (qBittorrent answers 403) logs in again once and repeats the request,
    /// so the caller does not see it.
    async fn call(
        &self,
        step: &str,
        method: Method,
        path: &str,
        build: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> Result<Response, QbError> {
        let url = format!("{}{path}", self.base_url);
        let mut stale: Option<String> = None;
        loop {
            let request =
                build(self.http.request(method.clone(), &url)?).header(REFERER, &self.base_url);
            let (request, used) = match &self.auth {
                QbAuth::ApiKey(key) => {
                    (request.header(AUTHORIZATION, format!("Bearer {key}")), None)
                }
                QbAuth::Password { username, password } => {
                    let cookie = self.cookie(username, password, stale.as_deref()).await?;
                    // No cookie: qBittorrent let this address in without a login.
                    if cookie.is_empty() {
                        (request, Some(cookie))
                    } else {
                        (request.header(COOKIE, cookie.clone()), Some(cookie))
                    }
                }
            };
            let response = request.send().await.map_err(unavailable)?;
            match (response.status(), &self.auth) {
                (status, _) if status.is_success() => return Ok(response),
                (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, QbAuth::ApiKey(_)) => {
                    return Err(QbError::ApiKey);
                }
                (StatusCode::FORBIDDEN, QbAuth::Password { .. }) if stale.is_none() => {
                    stale = used;
                }
                (StatusCode::FORBIDDEN, QbAuth::Password { .. }) => {
                    return Err(QbError::Credentials);
                }
                _ => return Err(unexpected(step, response).await),
            }
        }
    }

    /// The JSON answer of a call, or the call's error.
    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        step: &str,
        path: &str,
        build: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> Result<T, QbError> {
        let text = self
            .call(step, Method::GET, path, build)
            .await?
            .text()
            .await
            .map_err(unavailable)?;
        serde_json::from_str(&text)
            .map_err(|_| QbError::Unavailable(format!("{step} did not answer with JSON")))
    }

    /// Creates the category when the client has none of that name. The save
    /// path stays empty: the client decides where its torrents go.
    async fn ensure_category(&self, category: &str) -> Result<(), QbError> {
        let known: HashMap<String, serde_json::Value> = self
            .json("the categories call", "/api/v2/torrents/categories", |r| r)
            .await?;
        if known.contains_key(category) {
            return Ok(());
        }
        self.call(
            "the category creation",
            Method::POST,
            "/api/v2/torrents/createCategory",
            |r| r.form(&[("category", category)]),
        )
        .await?;
        Ok(())
    }

    /// Adds a magnet link or the URL of a torrent file under `category`,
    /// creating the category when it is missing. The save path is left to the
    /// client.
    ///
    /// Returns the info hash (lower case) of a magnet link. qBittorrent
    /// fetches a torrent URL itself and does not report the hash, so the
    /// answer is `None` and the caller finds the torrent by listing the
    /// category.
    pub async fn add(&self, category: &str, url: &str) -> Result<Option<String>, QbError> {
        check_category(category)?;
        let scheme = Url::parse(url).map(|url| url.scheme().to_owned());
        if !matches!(scheme.as_deref(), Ok("magnet" | "http" | "https")) {
            return Err(QbError::Unavailable(
                "only a magnet link or an http(s) torrent URL can be added".to_owned(),
            ));
        }
        self.ensure_category(category).await?;
        let response = self
            .call("the add call", Method::POST, "/api/v2/torrents/add", |r| {
                r.form(&[("urls", url), ("category", category)])
            })
            .await?;
        // qBittorrent answers 200 with `Ok.` or `Fails.`.
        if response.text().await.map_err(unavailable)?.trim() == "Fails." {
            return Err(QbError::Unavailable(
                "qBittorrent did not accept the torrent".to_owned(),
            ));
        }
        Ok(magnet_hash(url))
    }

    /// The torrents of `category`, and no others.
    pub async fn torrents(&self, category: &str) -> Result<Vec<Torrent>, QbError> {
        check_category(category)?;
        let mut torrents: Vec<Torrent> = self
            .json("the torrent list", "/api/v2/torrents/info", |r| {
                r.query(&[("category", category)])
            })
            .await?;
        // The filter is the client's; the check is ours, so that a client
        // that ignores it still leaks nothing.
        torrents.retain(|torrent| torrent.category == category);
        for torrent in &mut torrents {
            torrent.hash.make_ascii_lowercase();
        }
        Ok(torrents)
    }

    /// The properties of one torrent of `category`; `None` when the hash is
    /// not in that category.
    pub async fn properties(
        &self,
        category: &str,
        hash: &str,
    ) -> Result<Option<TorrentProperties>, QbError> {
        let hash = hash.to_ascii_lowercase();
        if !self
            .torrents(category)
            .await?
            .iter()
            .any(|t| t.hash == hash)
        {
            return Ok(None);
        }
        match self
            .json("the properties call", "/api/v2/torrents/properties", |r| {
                r.query(&[("hash", hash.as_str())])
            })
            .await
        {
            Ok(properties) => Ok(Some(properties)),
            // Removed between the list and this call.
            Err(QbError::Unavailable(reason)) if reason.contains("HTTP 404") => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Deletes the torrents of `category` among `hashes`, with their files
    /// when `delete_files` is set. A hash that is not in the category is
    /// skipped. Returns the hashes that were deleted.
    pub async fn delete(
        &self,
        category: &str,
        hashes: &[String],
        delete_files: bool,
    ) -> Result<Vec<String>, QbError> {
        let wanted: HashSet<String> = hashes.iter().map(|h| h.to_ascii_lowercase()).collect();
        // The hashes sent are the client's own, never the caller's text, so
        // `all` or a `|` list cannot widen the delete.
        let ours: Vec<String> = self
            .torrents(category)
            .await?
            .into_iter()
            .map(|torrent| torrent.hash)
            .filter(|hash| wanted.contains(hash))
            .collect();
        if ours.is_empty() {
            return Ok(ours);
        }
        let joined = ours.join("|");
        self.call(
            "the delete call",
            Method::POST,
            "/api/v2/torrents/delete",
            |r| {
                r.form(&[
                    ("hashes", joined.as_str()),
                    ("deleteFiles", if delete_files { "true" } else { "false" }),
                ])
            },
        )
        .await?;
        Ok(ours)
    }
}

/// An empty category matches the torrents that have none, which are not ours.
fn check_category(category: &str) -> Result<(), QbError> {
    if category.trim().is_empty() {
        return Err(QbError::Unavailable("the category is empty".to_owned()));
    }
    Ok(())
}

/// The info hash of a magnet link, lower case hex. The `btih` topic is 40 hex
/// digits or 32 base32 characters.
fn magnet_hash(link: &str) -> Option<String> {
    let link = Url::parse(link)
        .ok()
        .filter(|url| url.scheme() == "magnet")?;
    let topic = link.query_pairs().find_map(|(key, value)| {
        let prefix = value
            .get(..9)
            .filter(|p| p.eq_ignore_ascii_case("urn:btih:"))?;
        (key == "xt").then(|| value[prefix.len()..].to_owned())
    })?;
    match topic.len() {
        40 if topic.bytes().all(|b| b.is_ascii_hexdigit()) => Some(topic.to_ascii_lowercase()),
        32 => base32_to_hex(&topic),
        _ => None,
    }
}

fn base32_to_hex(text: &str) -> Option<String> {
    let (mut bits, mut count, mut hex) = (0u32, 0u32, String::new());
    for byte in text.bytes() {
        let value = match byte.to_ascii_uppercase() {
            letter @ b'A'..=b'Z' => letter - b'A',
            digit @ b'2'..=b'7' => digit - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u32::from(value);
        count += 5;
        if count >= 8 {
            count -= 8;
            write!(hex, "{:02x}", (bits >> count) & 0xff).ok()?;
            bits &= (1 << count) - 1;
        }
    }
    Some(hex)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn client(server: &MockServer, password: &str) -> QbClient {
        QbClient::new(
            OutboundClient::new().unwrap(),
            &format!("{}/", server.uri()),
            QbAuth::Password {
                username: "admin".into(),
                password: password.into(),
            },
        )
    }

    fn key_client(server: &MockServer, key: &str) -> QbClient {
        QbClient::new(
            OutboundClient::new().unwrap(),
            &server.uri(),
            QbAuth::ApiKey(key.into()),
        )
    }

    async fn login_answer(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_good_login_returns_the_version() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .and(body_string_contains("username=admin"))
            .and(body_string_contains("password=pa%26ss"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "SID=abc123; HttpOnly; path=/")
                    .set_body_string("Ok."),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .and(header("cookie", "SID=abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v4.6.2"))
            .mount(&server)
            .await;
        assert_eq!(
            client(&server, "pa&ss").test().await.as_deref(),
            Ok("v4.6.2")
        );
    }

    #[tokio::test]
    async fn a_200_with_fails_is_wrong_credentials() {
        let server = MockServer::start().await;
        login_answer(
            &server,
            ResponseTemplate::new(200).set_body_string("Fails."),
        )
        .await;
        assert_eq!(
            client(&server, "bad").test().await,
            Err(QbError::Credentials)
        );
    }

    #[tokio::test]
    async fn a_204_login_is_a_good_login_and_a_401_is_wrong_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(ResponseTemplate::new(204).insert_header("set-cookie", "SID=n1; path=/"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .and(header("cookie", "SID=n1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.1.0"))
            .mount(&server)
            .await;
        assert_eq!(
            client(&server, "good").test().await.as_deref(),
            Ok("v5.1.0")
        );

        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(401)).await;
        assert_eq!(
            client(&server, "bad").test().await,
            Err(QbError::Credentials)
        );
    }

    #[tokio::test]
    async fn a_403_on_login_is_a_ban() {
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(403)).await;
        assert_eq!(client(&server, "x").test().await, Err(QbError::Banned));
    }

    #[tokio::test]
    async fn another_server_is_not_qbittorrent() {
        let server = MockServer::start().await;
        login_answer(
            &server,
            ResponseTemplate::new(200).set_body_string("<html>"),
        )
        .await;
        assert!(matches!(
            client(&server, "x").test().await,
            Err(QbError::Unavailable(_))
        ));
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(404)).await;
        assert!(matches!(
            client(&server, "x").test().await,
            Err(QbError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn a_login_without_cookie_works_for_localhost_bypass() {
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(200).set_body_string("Ok.")).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.0.0"))
            .mount(&server)
            .await;
        assert_eq!(client(&server, "").test().await.as_deref(), Ok("v5.0.0"));
    }

    #[tokio::test]
    async fn a_refused_address_is_not_called() {
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://169.254.169.254",
            QbAuth::Password {
                username: "u".into(),
                password: "p".into(),
            },
        );
        assert_eq!(client.test().await, Err(QbError::Url(UrlError::Address)));
    }

    #[test]
    fn debug_hides_the_password_and_the_key() {
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://q",
            QbAuth::Password {
                username: "admin".into(),
                password: "hunter2".into(),
            },
        );
        assert!(!format!("{client:?}").contains("hunter2"));
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://q",
            QbAuth::ApiKey("qbt_secretsecretsecretsecret1234".into()),
        );
        assert!(!format!("{client:?}").contains("qbt_"));
    }

    #[tokio::test]
    async fn an_api_key_goes_in_a_bearer_header_and_no_login_is_made() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .and(header("authorization", "Bearer qbt_abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.2.0"))
            .mount(&server)
            .await;
        assert_eq!(
            key_client(&server, "qbt_abc").test().await.as_deref(),
            Ok("v5.2.0")
        );
    }

    #[tokio::test]
    async fn a_refused_api_key_is_named() {
        for status in [401, 403] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/v2/app/version"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            assert_eq!(
                key_client(&server, "qbt_bad").test().await,
                Err(QbError::ApiKey),
                "{status}"
            );
        }
    }

    #[tokio::test]
    async fn an_unexpected_answer_names_the_step_status_and_body() {
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(200).set_body_string("Ok.")).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .respond_with(ResponseTemplate::new(404).set_body_string(
                "Not
Found",
            ))
            .mount(&server)
            .await;
        let Err(QbError::Unavailable(reason)) = client(&server, "x").test().await else {
            panic!("expected an unavailable error");
        };
        assert_eq!(
            reason,
            "the version call answered HTTP 404 Not Found: Not Found"
        );

        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(500)).await;
        let Err(QbError::Unavailable(reason)) = client(&server, "x").test().await else {
            panic!("expected an unavailable error");
        };
        assert_eq!(reason, "the login answered HTTP 500 Internal Server Error");
    }

    const INFO: &str = include_str!("../../../fixtures/qbittorrent/torrents_info.json");
    const PROPERTIES: &str = include_str!("../../../fixtures/qbittorrent/torrent_properties.json");
    const CATEGORIES: &str = include_str!("../../../fixtures/qbittorrent/categories.json");

    const OURS: &str = "8c212779b4abde7c6bc608063a0d008b7e40ce32";
    const OURS_TOO: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const SONARR: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f001020304";
    const UNCATEGORISED: &str = "ffeeddccbbaa99887766554433221100ffeeddcc";

    /// A client that answers every login with `SID=abc123`.
    async fn logged_in(server: &MockServer) -> QbClient {
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "SID=abc123; HttpOnly; path=/")
                    .set_body_string("Ok."),
            )
            .mount(server)
            .await;
        client(server, "pw")
    }

    /// `torrents/info` answers the fixture, which holds torrents of other
    /// categories and one without a category, whatever the filter says.
    async fn info_answer(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/info"))
            .and(query_param("category", "unseenarr"))
            .and(header("cookie", "SID=abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string(INFO))
            .mount(server)
            .await;
    }

    async fn categories_answer(server: &MockServer, body: &str) {
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/categories"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(server)
            .await;
    }

    async fn requests_to(server: &MockServer, url_path: &str) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == url_path)
            .map(|request| String::from_utf8(request.body).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn the_list_holds_only_the_category_and_reads_the_content_path() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        info_answer(&server).await;
        let torrents = client.torrents("unseenarr").await.unwrap();
        let hashes: Vec<_> = torrents.iter().map(|t| t.hash.as_str()).collect();
        assert_eq!(hashes, [OURS, OURS_TOO]);
        assert_eq!(
            torrents[0].content_path,
            "/downloads/Movie.2024.1080p.BluRay.x264-GRP"
        );
        assert_eq!(torrents[0].state, "stalledUP");
        assert_eq!(torrents[1].progress, 0.5);
        assert_eq!(torrents[1].eta, 410);
    }

    #[tokio::test]
    async fn an_empty_category_is_refused_before_any_call() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        assert!(client.torrents("").await.is_err());
        assert!(client.add(" ", "magnet:?xt=urn:btih:abc").await.is_err());
        assert!(
            client
                .delete("", &[UNCATEGORISED.into()], true)
                .await
                .is_err()
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn properties_answer_for_our_torrent_and_not_for_another_category() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        info_answer(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/properties"))
            .and(query_param("hash", OURS))
            .respond_with(ResponseTemplate::new(200).set_body_string(PROPERTIES))
            .expect(1)
            .mount(&server)
            .await;
        let properties = client
            .properties("unseenarr", &OURS.to_ascii_uppercase())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(properties.save_path, "/downloads");
        assert_eq!(properties.total_size, 4_294_967_296);
        assert_eq!(properties.completion_date, 1_735_703_600);
        // Another category and no category: no properties call at all.
        for other in [SONARR, UNCATEGORISED, "unknown"] {
            assert_eq!(
                client.properties("unseenarr", other).await,
                Ok(None),
                "{other}"
            );
        }
    }

    #[tokio::test]
    async fn a_magnet_creates_the_missing_category_and_leaves_the_save_path() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        categories_answer(
            &server,
            r#"{"tv-sonarr":{"name":"tv-sonarr","savePath":"/tv"}}"#,
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/v2/torrents/createCategory"))
            .and(body_string_contains("category=unseenarr"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v2/torrents/add"))
            .and(header("cookie", "SID=abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Ok."))
            .expect(1)
            .mount(&server)
            .await;
        let magnet = format!("magnet:?xt=urn:btih:{}&dn=Movie", OURS.to_ascii_uppercase());
        assert_eq!(
            client.add("unseenarr", &magnet).await,
            Ok(Some(OURS.to_owned()))
        );
        let added = requests_to(&server, "/api/v2/torrents/add").await;
        assert_eq!(added.len(), 1);
        assert!(added[0].contains("category=unseenarr"));
        assert!(added[0].contains("urls=magnet%3A%3Fxt%3Durn%3Abtih%3A"));
        assert!(!added[0].to_ascii_lowercase().contains("savepath"));
        let created = requests_to(&server, "/api/v2/torrents/createCategory").await;
        assert!(!created[0].to_ascii_lowercase().contains("savepath"));
    }

    #[tokio::test]
    async fn an_existing_category_is_not_created_again() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        categories_answer(&server, CATEGORIES).await;
        Mock::given(method("POST"))
            .and(path("/api/v2/torrents/createCategory"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v2/torrents/add"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Ok."))
            .mount(&server)
            .await;
        // A torrent URL: qBittorrent fetches it, so there is no hash yet.
        assert_eq!(
            client
                .add(
                    "unseenarr",
                    "https://tracker.example.org/download/1.torrent?key=k"
                )
                .await,
            Ok(None)
        );
    }

    #[tokio::test]
    async fn a_refused_or_unsupported_add_is_an_error() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        categories_answer(&server, CATEGORIES).await;
        Mock::given(method("POST"))
            .and(path("/api/v2/torrents/add"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Fails."))
            .mount(&server)
            .await;
        assert!(matches!(
            client
                .add("unseenarr", "https://example.org/1.torrent")
                .await,
            Err(QbError::Unavailable(_))
        ));
        // A file path is never passed on to the client.
        assert!(client.add("unseenarr", "file:///etc/passwd").await.is_err());
        assert!(client.add("unseenarr", "not a url").await.is_err());
        assert_eq!(requests_to(&server, "/api/v2/torrents/add").await.len(), 1);
    }

    #[tokio::test]
    async fn delete_touches_only_our_torrents_and_reports_them() {
        for delete_files in [true, false] {
            let server = MockServer::start().await;
            let client = logged_in(&server).await;
            info_answer(&server).await;
            Mock::given(method("POST"))
                .and(path("/api/v2/torrents/delete"))
                .respond_with(ResponseTemplate::new(200))
                .expect(1)
                .mount(&server)
                .await;
            let asked: Vec<String> = [OURS, SONARR, UNCATEGORISED, "all", "x|y"]
                .map(String::from)
                .into();
            assert_eq!(
                client.delete("unseenarr", &asked, delete_files).await,
                Ok(vec![OURS.to_owned()])
            );
            let sent = requests_to(&server, "/api/v2/torrents/delete").await;
            assert_eq!(sent, [format!("hashes={OURS}&deleteFiles={delete_files}")]);
        }
    }

    #[tokio::test]
    async fn delete_without_a_torrent_of_ours_sends_nothing() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        info_answer(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/v2/torrents/delete"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let asked = [SONARR.to_owned(), UNCATEGORISED.to_owned()];
        assert_eq!(client.delete("unseenarr", &asked, true).await, Ok(vec![]));
    }

    #[tokio::test]
    async fn an_expired_session_logs_in_again_without_failing_the_call() {
        let server = MockServer::start().await;
        // The first login hands out a cookie that qBittorrent then forgets.
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "SID=old; path=/")
                    .set_body_string("Ok."),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "SID=new; path=/")
                    .set_body_string("Ok."),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/info"))
            .and(header("cookie", "SID=old"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/info"))
            .and(header("cookie", "SID=new"))
            .respond_with(ResponseTemplate::new(200).set_body_string(INFO))
            .mount(&server)
            .await;
        let client = client(&server, "pw");
        assert_eq!(client.torrents("unseenarr").await.unwrap().len(), 2);
        // The new cookie is kept: no third login.
        assert_eq!(client.torrents("unseenarr").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_session_is_reused_between_calls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "SID=abc123; path=/")
                    .set_body_string("Ok."),
            )
            .expect(1)
            .mount(&server)
            .await;
        info_answer(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.0.0"))
            .mount(&server)
            .await;
        let client = client(&server, "pw");
        client.torrents("unseenarr").await.unwrap();
        client.torrents("unseenarr").await.unwrap();
        assert_eq!(client.test().await.as_deref(), Ok("v5.0.0"));
    }

    #[tokio::test]
    async fn a_403_after_a_fresh_login_is_wrong_credentials() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/info"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        assert_eq!(
            client.torrents("unseenarr").await,
            Err(QbError::Credentials)
        );
        // One retry, not a loop: two logins in all.
        assert_eq!(requests_to(&server, "/api/v2/auth/login").await.len(), 2);
    }

    #[tokio::test]
    async fn an_api_key_client_lists_with_the_bearer_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/info"))
            .and(header("authorization", "Bearer qbt_abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(INFO))
            .mount(&server)
            .await;
        let client = key_client(&server, "qbt_abc");
        assert_eq!(client.torrents("unseenarr").await.unwrap().len(), 2);
        assert!(requests_to(&server, "/api/v2/auth/login").await.is_empty());
    }

    #[tokio::test]
    async fn a_list_that_is_not_json_is_unavailable() {
        let server = MockServer::start().await;
        let client = logged_in(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/torrents/info"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        assert_eq!(
            client.torrents("unseenarr").await,
            Err(QbError::Unavailable(
                "the torrent list did not answer with JSON".to_owned()
            ))
        );
    }

    #[test]
    fn a_magnet_hash_is_hex_or_base32() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        let base32 = "AERUKZ4JVPG66AJDIVTYTK6N54ASGRLH";
        for link in [
            format!("magnet:?xt=urn:btih:{hex}&dn=x"),
            format!("magnet:?dn=x&xt=urn:BTIH:{}", hex.to_ascii_uppercase()),
            format!("magnet:?xt=urn:btih:{base32}"),
            format!("magnet:?xt=urn:btih:{}", base32.to_ascii_lowercase()),
        ] {
            assert_eq!(magnet_hash(&link).as_deref(), Some(hex), "{link}");
        }
        for link in [
            "magnet:?xt=urn:btmh:1220abcd",
            "magnet:?xt=urn:btih:zz",
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef0123456g",
            "https://example.org/?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            "nonsense",
        ] {
            assert_eq!(magnet_hash(link), None, "{link}");
        }
    }
}
