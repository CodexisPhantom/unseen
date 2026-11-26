//! The SABnzbd API client (docs/08-integrations.md): the version call that
//! the test button uses, and the NZB calls (add, queue, history, delete).
//!
//! The queue and history calls follow the category of the download client.
//! A slot in another category is never listed and never touched, even when
//! its id is given. The client decides where it saves its releases; Unseen
//! reads `storage` from the history.

use std::collections::HashSet;
use std::fmt;

use reqwest::StatusCode;
use serde::Deserialize;
use url::Url;

use crate::http::{OutboundClient, UrlError};

#[derive(Debug, PartialEq, Eq)]
pub enum SabError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// SABnzbd answered and refused the API key (HTTP 401/403).
    ApiKey,
    /// No answer, or an answer that is not the SABnzbd API.
    Unavailable(String),
}

impl fmt::Display for SabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "{error}"),
            Self::ApiKey => f.write_str("SABnzbd refused the API key"),
            Self::Unavailable(reason) => write!(f, "SABnzbd is not available: {reason}"),
        }
    }
}

impl std::error::Error for SabError {}

impl From<UrlError> for SabError {
    fn from(error: UrlError) -> Self {
        Self::Url(error)
    }
}

/// A queue slot of the category, as `mode=queue` lists it.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QueueItem {
    pub nzo_id: String,
    pub filename: String,
    pub cat: String,
    pub status: String,
    /// 0 to 100, as SABnzbd sends it (a string).
    pub percentage: String,
    /// Size in MB, as a string.
    pub mb: String,
    /// Remaining MB, as a string.
    pub mbleft: String,
}

/// A history slot of the category, as `mode=history` lists it.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct HistoryItem {
    pub nzo_id: String,
    pub name: String,
    pub category: String,
    pub status: String,
    /// Bytes.
    pub bytes: i64,
    /// The folder SABnzbd saved the release to, as SABnzbd sees it.
    pub storage: String,
    pub path: String,
    /// Why it failed; empty otherwise.
    pub fail_message: String,
}

pub struct SabClient {
    http: OutboundClient,
    base_url: String,
    api_key: String,
}

// Never shows the key.
impl fmt::Debug for SabClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SabClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// The step of the call, so an error says where it happened. The body is the
/// first words of a text answer.
async fn unexpected(step: &str, response: reqwest::Response) -> SabError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let body: String = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect();
    SabError::Unavailable(if body.is_empty() {
        format!("{step} answered HTTP {status}")
    } else {
        format!("{step} answered HTTP {status}: {body}")
    })
}

fn unavailable(error: reqwest::Error) -> SabError {
    // `without_url`: the reason must not echo the address with credentials.
    SabError::Unavailable(error.without_url().to_string())
}

/// True when the error text names the API key.
fn is_api_key_error(text: &str) -> bool {
    text.to_ascii_lowercase().contains("api key")
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct QueueWrap {
    queue: QueueInner,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct QueueInner {
    slots: Vec<QueueItem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HistoryWrap {
    history: HistoryInner,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HistoryInner {
    slots: Vec<HistoryItem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VersionAnswer {
    version: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AddAnswer {
    nzo_ids: Vec<String>,
}

impl SabClient {
    /// `base_url` is the SABnzbd address, for example `http://sabnzbd:8080`.
    pub fn new(http: OutboundClient, base_url: &str, api_key: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.to_owned(),
        }
    }

    /// One GET to `{base_url}/api` with `output=json`, `apikey={key}` and
    /// the call's params. Returns the JSON answer, or the call's error.
    async fn api(
        &self,
        step: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value, SabError> {
        let url = format!("{}/api", self.base_url);
        let mut request = self.http.get(&url)?;
        request = request.query(&[("output", "json"), ("apikey", self.api_key.as_str())]);
        for (key, value) in params {
            request = request.query(&[(key, value)]);
        }
        let response = request.send().await.map_err(unavailable)?;
        match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(SabError::ApiKey),
            status if !status.is_success() => return Err(unexpected(step, response).await),
            _ => {}
        }
        let text = response.text().await.map_err(unavailable)?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| SabError::Unavailable(format!("{step} did not answer with JSON")))?;
        if json.get("status").and_then(|s| s.as_bool()) == Some(false) {
            let error = json
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or_default();
            if is_api_key_error(error) {
                return Err(SabError::ApiKey);
            }
            if error.is_empty() {
                return Err(SabError::Unavailable(format!("{step} reported a failure")));
            }
            return Err(SabError::Unavailable(format!("{step}: {error}")));
        }
        Ok(json)
    }

    /// Authenticates with the key and returns the version, e.g. "4.3.2".
    pub async fn test(&self) -> Result<String, SabError> {
        // The version call does not require the key, so the queue call runs
        // first: it rejects a wrong key.
        self.api("the queue call", &[("mode", "queue"), ("limit", "1")])
            .await?;
        let json = self.api("the version call", &[("mode", "version")]).await?;
        let answer: VersionAnswer = serde_json::from_value(json).map_err(|_| {
            SabError::Unavailable("the version call did not answer with JSON".to_owned())
        })?;
        Ok(answer.version)
    }

    /// Adds the http(s) URL of an NZB under `category`, with an optional
    /// display name, and returns the first nzo id SABnzbd assigned.
    pub async fn add(
        &self,
        category: &str,
        url: &str,
        name: Option<&str>,
    ) -> Result<Option<String>, SabError> {
        check_category(category)?;
        let scheme = Url::parse(url).map(|url| url.scheme().to_owned());
        if !matches!(scheme.as_deref(), Ok("http" | "https")) {
            return Err(SabError::Unavailable(
                "only an http(s) link to an NZB can be added".to_owned(),
            ));
        }
        let nzbname = name.filter(|n| !n.trim().is_empty());
        let mut params = vec![("mode", "addurl"), ("name", url), ("cat", category)];
        if let Some(nzbname) = nzbname {
            params.push(("nzbname", nzbname));
        }
        let json = self.api("the add call", &params).await?;
        let answer: AddAnswer = serde_json::from_value(json).map_err(|_| {
            SabError::Unavailable("the add call did not answer with JSON".to_owned())
        })?;
        Ok(answer.nzo_ids.into_iter().next())
    }

    /// The queue slots of `category`, and no others.
    pub async fn queue(&self, category: &str) -> Result<Vec<QueueItem>, SabError> {
        check_category(category)?;
        let json = self
            .api("the queue call", &[("mode", "queue"), ("cat", category)])
            .await?;
        let wrap: QueueWrap = serde_json::from_value(json).map_err(|_| {
            SabError::Unavailable("the queue call did not answer with JSON".to_owned())
        })?;
        Ok(wrap
            .queue
            .slots
            .into_iter()
            .filter(|slot| slot.cat == category)
            .collect())
    }

    /// The history slots of `category`, and no others.
    pub async fn history(&self, category: &str) -> Result<Vec<HistoryItem>, SabError> {
        check_category(category)?;
        let json = self
            .api(
                "the history call",
                &[("mode", "history"), ("cat", category)],
            )
            .await?;
        let wrap: HistoryWrap = serde_json::from_value(json).map_err(|_| {
            SabError::Unavailable("the history call did not answer with JSON".to_owned())
        })?;
        Ok(wrap
            .history
            .slots
            .into_iter()
            .filter(|slot| slot.category == category)
            .collect())
    }

    /// Deletes the given ids that are in `category`, from the queue or the
    /// history, and returns the ids actually deleted (queue first).
    pub async fn delete(
        &self,
        category: &str,
        ids: &[String],
        delete_files: bool,
    ) -> Result<Vec<String>, SabError> {
        check_category(category)?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let queue_ids: Vec<String> = self
            .queue(category)
            .await?
            .into_iter()
            .map(|slot| slot.nzo_id)
            .filter(|id| wanted.contains(id.as_str()))
            .collect();
        let history_ids: Vec<String> = self
            .history(category)
            .await?
            .into_iter()
            .map(|slot| slot.nzo_id)
            .filter(|id| wanted.contains(id.as_str()))
            .collect();
        let flag = if delete_files { "1" } else { "0" };
        for id in &queue_ids {
            self.api(
                "the delete call",
                &[
                    ("mode", "queue"),
                    ("name", "delete"),
                    ("value", id.as_str()),
                    ("del_files", flag),
                ],
            )
            .await?;
        }
        for id in &history_ids {
            self.api(
                "the delete call",
                &[
                    ("mode", "history"),
                    ("name", "delete"),
                    ("value", id.as_str()),
                    ("archive", "0"),
                    ("del_files", flag),
                ],
            )
            .await?;
        }
        Ok(queue_ids.into_iter().chain(history_ids).collect())
    }
}

/// An empty category matches the slots that have none, which are not ours.
fn check_category(category: &str) -> Result<(), SabError> {
    if category.trim().is_empty() {
        return Err(SabError::Unavailable("the category is empty".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::http::UrlError;

    const QUEUE: &str = include_str!("../../../fixtures/sabnzbd/queue.json");
    const HISTORY: &str = include_str!("../../../fixtures/sabnzbd/history.json");
    const VERSION: &str = include_str!("../../../fixtures/sabnzbd/version.json");
    const ADDURL: &str = include_str!("../../../fixtures/sabnzbd/addurl.json");

    fn client(server: &MockServer) -> SabClient {
        SabClient::new(
            OutboundClient::new().unwrap(),
            &format!("{}/", server.uri()),
            "secret",
        )
    }

    #[tokio::test]
    async fn test_validates_the_key_and_reports_the_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "queue"))
            .respond_with(ResponseTemplate::new(200).set_body_string(QUEUE))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(VERSION))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(client(&server).test().await.as_deref(), Ok("4.3.2"));
    }

    #[tokio::test]
    async fn a_refused_key_is_named_and_html_is_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .respond_with(ResponseTemplate::new(401).set_body_string("error: API Key Incorrect"))
            .mount(&server)
            .await;
        assert_eq!(client(&server).test().await, Err(SabError::ApiKey));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        assert!(matches!(
            client(&server).test().await,
            Err(SabError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn add_sends_the_url_category_and_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "addurl"))
            .and(query_param(
                "name",
                "https://indexer.example/dl/1.nzb?key=k",
            ))
            .and(query_param("cat", "radarr"))
            .and(query_param("nzbname", "Movie.2024"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ADDURL))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            client(&server)
                .add(
                    "radarr",
                    "https://indexer.example/dl/1.nzb?key=k",
                    Some("Movie.2024")
                )
                .await,
            Ok(Some("SABnzbd_nzo_kyt1f0".to_owned()))
        );

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "addurl"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"status": false, "nzo_ids": []}"#),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            client(&server)
                .add("radarr", "https://indexer.example/dl/1.nzb", None)
                .await,
            Err(SabError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn add_rejects_an_empty_category_and_a_non_http_url() {
        let server = MockServer::start().await;
        let client = client(&server);
        assert!(
            client
                .add("", "https://example.org/1.nzb", None)
                .await
                .is_err()
        );
        assert!(
            client
                .add(" ", "https://example.org/1.nzb", None)
                .await
                .is_err()
        );
        assert!(
            client
                .add("radarr", "magnet:?xt=urn:btih:abc", None)
                .await
                .is_err()
        );
        assert!(
            client
                .add("radarr", "file:///etc/passwd", None)
                .await
                .is_err()
        );
        assert!(client.add("radarr", "not a url", None).await.is_err());
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn queue_holds_only_the_category_and_sends_it() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "queue"))
            .and(query_param("cat", "radarr"))
            .respond_with(ResponseTemplate::new(200).set_body_string(QUEUE))
            .expect(1)
            .mount(&server)
            .await;
        let slots = client(&server).queue("radarr").await.unwrap();
        let ids: Vec<_> = slots.iter().map(|s| s.nzo_id.as_str()).collect();
        assert_eq!(ids, ["SABnzbd_nzo_kyt1f0", "SABnzbd_nzo_kyt1f1"]);
        assert_eq!(slots[0].percentage, "42");
        assert_eq!(slots[0].mb, "1024.00");
    }

    #[tokio::test]
    async fn history_holds_only_the_category_and_sends_it() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "history"))
            .and(query_param("cat", "radarr"))
            .respond_with(ResponseTemplate::new(200).set_body_string(HISTORY))
            .expect(1)
            .mount(&server)
            .await;
        let slots = client(&server).history("radarr").await.unwrap();
        let ids: Vec<_> = slots.iter().map(|s| s.nzo_id.as_str()).collect();
        assert_eq!(ids, ["SABnzbd_nzo_h1", "SABnzbd_nzo_h3"]);
        assert_eq!(slots[0].bytes, 4_294_967_296);
    }

    #[tokio::test]
    async fn delete_touches_only_our_ids_and_reports_them() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "queue"))
            .and(query_param("cat", "radarr"))
            .respond_with(ResponseTemplate::new(200).set_body_string(QUEUE))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "history"))
            .and(query_param("cat", "radarr"))
            .respond_with(ResponseTemplate::new(200).set_body_string(HISTORY))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "queue"))
            .and(query_param("name", "delete"))
            .and(query_param("value", "SABnzbd_nzo_kyt1f0"))
            .and(query_param("del_files", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status": true}"#))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(query_param("mode", "history"))
            .and(query_param("name", "delete"))
            .and(query_param("value", "SABnzbd_nzo_h3"))
            .and(query_param("archive", "0"))
            .and(query_param("del_files", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status": true}"#))
            .expect(1)
            .mount(&server)
            .await;
        // The tv ids must provoke no delete request.
        for id in ["SABnzbd_nzo_kyt1f2", "SABnzbd_nzo_h2"] {
            Mock::given(method("GET"))
                .and(path("/api"))
                .and(query_param("value", id))
                .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status": true}"#))
                .expect(0)
                .mount(&server)
                .await;
        }
        let asked = [
            "SABnzbd_nzo_kyt1f0",
            "SABnzbd_nzo_h3",
            "SABnzbd_nzo_kyt1f2",
            "SABnzbd_nzo_h2",
        ]
        .map(String::from);
        assert_eq!(
            client(&server).delete("radarr", &asked, true).await,
            Ok(vec![
                "SABnzbd_nzo_kyt1f0".to_owned(),
                "SABnzbd_nzo_h3".to_owned()
            ])
        );
    }

    #[tokio::test]
    async fn an_empty_id_list_makes_no_call() {
        let server = MockServer::start().await;
        let uri = server.uri();
        drop(server);
        let client = SabClient::new(OutboundClient::new().unwrap(), &uri, "secret");
        assert_eq!(client.delete("radarr", &[], true).await, Ok(vec![]));
    }

    #[test]
    fn debug_hides_the_key() {
        let client = SabClient::new(
            OutboundClient::new().unwrap(),
            "http://sab",
            "supersecretkey",
        );
        let shown = format!("{client:?}");
        assert!(!shown.contains("supersecretkey"), "{shown}");
        assert!(shown.contains("http://sab"));
    }

    #[tokio::test]
    async fn a_refused_address_is_not_called() {
        let client = SabClient::new(
            OutboundClient::new().unwrap(),
            "http://169.254.169.254",
            "secret",
        );
        assert_eq!(client.test().await, Err(SabError::Url(UrlError::Address)));
    }
}
