//! The Torznab client (docs/08-integrations.md): `t=caps`, movie search,
//! text search and the latest-releases feed.
//!
//! Use only the search modes and parameters that `caps` advertises. A movie
//! is searched by IMDb or TMDB id when the indexer supports one of them,
//! otherwise by text. The API key travels in the `apikey` query parameter, so
//! no error text may carry the request URL.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use roxmltree::{Document, Node};

use crate::http::{OutboundClient, UrlError};

#[derive(Debug, PartialEq, Eq)]
pub enum TorznabError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// The indexer answered, and refused the API key (error codes 100 to 109,
    /// or HTTP 401 and 403).
    ApiKey,
    /// The indexer answered with an `<error>` element.
    Indexer { code: u32, description: String },
    /// HTTP 429. The caller backs off for `retry_after`, when the indexer
    /// said how long.
    RateLimited { retry_after: Option<Duration> },
    /// No answer, or an answer that is not Torznab.
    Unavailable(String),
}

impl fmt::Display for TorznabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "{error}"),
            Self::ApiKey => f.write_str("the indexer refused the API key"),
            Self::Indexer { code, description } => {
                write!(f, "the indexer answered error {code}: {description}")
            }
            Self::RateLimited {
                retry_after: Some(wait),
            } => write!(
                f,
                "the indexer is rate limiting; retry in {} s",
                wait.as_secs()
            ),
            Self::RateLimited { retry_after: None } => f.write_str("the indexer is rate limiting"),
            Self::Unavailable(reason) => write!(f, "the indexer is not available: {reason}"),
        }
    }
}

impl std::error::Error for TorznabError {}

impl From<UrlError> for TorznabError {
    fn from(error: UrlError) -> Self {
        Self::Url(error)
    }
}

/// One search mode of `caps` and the parameters it accepts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchMode {
    pub params: Vec<String>,
}

impl SearchMode {
    pub fn supports(&self, param: &str) -> bool {
        self.params.iter().any(|p| p == param)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: u32,
    pub name: String,
}

/// What an indexer says it can do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Caps {
    /// `None` when the mode is missing or `available="no"`.
    pub search: Option<SearchMode>,
    pub movie_search: Option<SearchMode>,
    /// Categories and sub-categories in one list.
    pub categories: Vec<Category>,
}

/// One result item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub title: String,
    pub guid: String,
    pub size: Option<u64>,
    /// The `.torrent` file address (enclosure, else `<link>`).
    pub download_url: Option<String>,
    pub magnet_url: Option<String>,
    /// Lower case hex.
    pub info_hash: Option<String>,
    pub seeders: Option<u32>,
    pub peers: Option<u32>,
    pub published: Option<DateTime<Utc>>,
    pub categories: Vec<u32>,
}

/// What to look for.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MovieQuery {
    pub tmdb_id: Option<i32>,
    /// With or without the `tt` prefix.
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i32>,
    /// Torznab category ids; empty searches every category.
    pub categories: Vec<i32>,
}

pub struct TorznabClient {
    http: OutboundClient,
    base_url: String,
    api_key: String,
}

// No derive: the key must not reach a log line.
impl fmt::Debug for TorznabClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TorznabClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

type Params = Vec<(&'static str, String)>;

fn unavailable(error: reqwest::Error) -> TorznabError {
    // `without_url`: the URL carries the API key.
    TorznabError::Unavailable(error.without_url().to_string())
}

fn not_torznab() -> TorznabError {
    TorznabError::Unavailable("the address does not answer like a Torznab indexer".to_owned())
}

/// The first words of a body, to tell an indexer message from a proxy page.
fn snippet(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

/// The error of an `<error code= description=/>` document.
fn error_element(root: Node) -> Option<TorznabError> {
    if root.tag_name().name() != "error" {
        return None;
    }
    let code = root
        .attribute("code")
        .and_then(|code| code.parse().ok())
        .unwrap_or(900);
    Some(if (100..110).contains(&code) {
        TorznabError::ApiKey
    } else {
        TorznabError::Indexer {
            code,
            description: root.attribute("description").unwrap_or_default().to_owned(),
        }
    })
}

fn parse_document(xml: &str) -> Result<Document<'_>, TorznabError> {
    Document::parse(xml).map_err(|_| not_torznab())
}

fn mode(searching: Node, name: &str) -> Option<SearchMode> {
    let node = searching.children().find(|n| n.has_tag_name(name))?;
    if node.attribute("available") != Some("yes") {
        return None;
    }
    Some(SearchMode {
        params: node
            .attribute("supportedParams")
            .unwrap_or_default()
            .split(',')
            .map(|param| param.trim().to_owned())
            .filter(|param| !param.is_empty())
            .collect(),
    })
}

/// Parses a `t=caps` answer.
pub fn parse_caps(xml: &str) -> Result<Caps, TorznabError> {
    let document = parse_document(xml)?;
    let root = document.root_element();
    if let Some(error) = error_element(root) {
        return Err(error);
    }
    if root.tag_name().name() != "caps" {
        return Err(not_torznab());
    }
    let searching = root.children().find(|n| n.has_tag_name("searching"));
    let categories = root
        .descendants()
        .filter(|n| n.has_tag_name("category") || n.has_tag_name("subcat"))
        .filter_map(|n| {
            Some(Category {
                id: n.attribute("id")?.parse().ok()?,
                name: n.attribute("name")?.to_owned(),
            })
        })
        .collect();
    Ok(Caps {
        search: searching.and_then(|s| mode(s, "search")),
        movie_search: searching.and_then(|s| mode(s, "movie-search")),
        categories,
    })
}

fn text(item: Node, name: &str) -> Option<String> {
    item.children()
        .find(|n| n.has_tag_name(name))
        .and_then(|n| n.text())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// `<torznab:attr name= value=/>` values for one name; the namespace prefix
/// does not matter, so `newznab:attr` reads the same.
fn attrs<'a>(item: Node<'a, 'a>, name: &'a str) -> impl Iterator<Item = &'a str> {
    item.children()
        .filter(|n| n.has_tag_name("attr") && n.attribute("name") == Some(name))
        .filter_map(|n| n.attribute("value"))
}

fn release(item: Node) -> Option<Release> {
    let title = text(item, "title")?;
    let enclosure = item
        .children()
        .find(|n| n.has_tag_name("enclosure"))
        .and_then(|n| n.attribute("url"))
        .map(str::to_owned);
    let link = text(item, "link");
    let mut magnet_url = attrs(item, "magneturl").next().map(str::to_owned);
    let mut download_url = None;
    // A magnet may sit in the enclosure or the link instead of the attribute.
    for url in [enclosure, link].into_iter().flatten() {
        if url.starts_with("magnet:") {
            magnet_url.get_or_insert(url);
        } else if download_url.is_none() {
            download_url = Some(url);
        }
    }
    if download_url.is_none() && magnet_url.is_none() {
        return None;
    }
    let number = |name: &str| attrs(item, name).next().and_then(|v| v.parse::<u64>().ok());
    let mut categories: Vec<u32> = attrs(item, "category")
        .filter_map(|v| v.parse().ok())
        .collect();
    if categories.is_empty() {
        categories = item
            .children()
            .filter(|n| n.has_tag_name("category"))
            .filter_map(|n| n.text()?.trim().parse().ok())
            .collect();
    }
    Some(Release {
        guid: text(item, "guid").unwrap_or_else(|| title.clone()),
        size: text(item, "size")
            .and_then(|size| size.parse().ok())
            .or_else(|| number("size"))
            .or_else(|| {
                item.children()
                    .find(|n| n.has_tag_name("enclosure"))
                    .and_then(|n| n.attribute("length")?.parse().ok())
            }),
        info_hash: attrs(item, "infohash")
            .next()
            .map(str::to_ascii_lowercase)
            .filter(|hash| !hash.is_empty()),
        seeders: number("seeders").and_then(|n| u32::try_from(n).ok()),
        peers: number("peers").and_then(|n| u32::try_from(n).ok()),
        published: text(item, "pubDate")
            .and_then(|date| DateTime::parse_from_rfc2822(&date).ok())
            .map(|date| date.with_timezone(&Utc)),
        categories,
        download_url,
        magnet_url,
        title,
    })
}

/// Parses a search or feed answer. An item without a title or without any
/// download address is skipped, because it cannot be grabbed.
pub fn parse_feed(xml: &str) -> Result<Vec<Release>, TorznabError> {
    let document = parse_document(xml)?;
    let root = document.root_element();
    if let Some(error) = error_element(root) {
        return Err(error);
    }
    if root.tag_name().name() != "rss" {
        return Err(not_torznab());
    }
    Ok(root
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(release)
        .collect())
}

/// The first of IMDb and TMDB id that the query has and the mode supports.
fn id_param(mode: &SearchMode, query: &MovieQuery) -> Option<(&'static str, String)> {
    let imdb = query
        .imdb_id
        .as_deref()
        .map(|id| id.trim_start_matches("tt"))
        .filter(|id| !id.is_empty() && mode.supports("imdbid"));
    if let Some(id) = imdb {
        return Some(("imdbid", id.to_owned()));
    }
    query
        .tmdb_id
        .filter(|_| mode.supports("tmdbid"))
        .map(|id| ("tmdbid", id.to_string()))
}

fn categories(ids: &[i32]) -> Option<(&'static str, String)> {
    (!ids.is_empty()).then(|| {
        let ids: Vec<String> = ids.iter().map(i32::to_string).collect();
        ("cat", ids.join(","))
    })
}

/// The query parameters of a movie search, from what `caps` advertises.
pub fn movie_params(caps: &Caps, query: &MovieQuery) -> Result<Params, TorznabError> {
    let mut params: Params = Vec::new();
    let title = query.title.trim();
    if let Some(mode) = &caps.movie_search {
        if let Some(id) = id_param(mode, query) {
            params.push(("t", "movie".to_owned()));
            params.push(id);
        } else if mode.supports("q") && !title.is_empty() {
            params.push(("t", "movie".to_owned()));
            params.push(("q", title.to_owned()));
            if let Some(year) = query.year.filter(|_| mode.supports("year")) {
                params.push(("year", year.to_string()));
            }
        }
    }
    if params.is_empty() {
        let supports_text = caps.search.as_ref().is_some_and(|mode| mode.supports("q"));
        if !supports_text || title.is_empty() {
            return Err(TorznabError::Unavailable(
                "the indexer cannot search for this movie".to_owned(),
            ));
        }
        let text = match query.year {
            Some(year) => format!("{title} {year}"),
            None => title.to_owned(),
        };
        params.push(("t", "search".to_owned()));
        params.push(("q", text));
    }
    params.extend(categories(&query.categories));
    Ok(params)
}

/// The query parameters of the latest-releases feed: a movie search without
/// a query, or a plain search when the indexer has no movie mode.
pub fn feed_params(caps: &Caps, category_ids: &[i32]) -> Result<Params, TorznabError> {
    let function = match (&caps.movie_search, &caps.search) {
        (Some(_), _) => "movie",
        (None, Some(_)) => "search",
        (None, None) => {
            return Err(TorznabError::Unavailable(
                "the indexer advertises no search".to_owned(),
            ));
        }
    };
    let mut params: Params = vec![("t", function.to_owned())];
    params.extend(categories(category_ids));
    Ok(params)
}

impl TorznabClient {
    /// `base_url` is the Torznab address, for example
    /// `http://jackett:9117/api/v2.0/indexers/all/results/torznab/`.
    pub fn new(http: OutboundClient, base_url: &str, api_key: &str) -> Self {
        Self {
            http,
            base_url: base_url.to_owned(),
            api_key: api_key.to_owned(),
        }
    }

    async fn get(&self, params: &[(&'static str, String)]) -> Result<String, TorznabError> {
        let response = self
            .http
            .request(Method::GET, &self.base_url)?
            .query(&[("apikey", self.api_key.as_str())])
            .query(params)
            .send()
            .await
            .map_err(unavailable)?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok())
            .map(Duration::from_secs);
        let body = response.text().await.map_err(unavailable)?;
        // Indexers put the error element on 200 and on 4xx alike.
        if let Ok(document) = Document::parse(&body)
            && let Some(error) = error_element(document.root_element())
        {
            return Err(error);
        }
        match status {
            StatusCode::TOO_MANY_REQUESTS => Err(TorznabError::RateLimited { retry_after }),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(TorznabError::ApiKey),
            status if status.is_success() => Ok(body),
            status => Err(TorznabError::Unavailable(format!(
                "answered HTTP {status}: {}",
                snippet(&body)
            ))),
        }
    }

    pub async fn caps(&self) -> Result<Caps, TorznabError> {
        parse_caps(&self.get(&[("t", "caps".to_owned())]).await?)
    }

    pub async fn search_movie(
        &self,
        caps: &Caps,
        query: &MovieQuery,
    ) -> Result<Vec<Release>, TorznabError> {
        parse_feed(&self.get(&movie_params(caps, query)?).await?)
    }

    /// The newest releases, for RSS sync.
    pub async fn latest(
        &self,
        caps: &Caps,
        category_ids: &[i32],
    ) -> Result<Vec<Release>, TorznabError> {
        parse_feed(&self.get(&feed_params(caps, category_ids)?).await?)
    }

    /// Reads `caps` and then asks for the feed with `limit=1`, because some
    /// indexers answer `caps` without a key but refuse a search. Returns the
    /// caps.
    pub async fn test(&self) -> Result<Caps, TorznabError> {
        let caps = self.caps().await?;
        let mut params = feed_params(&caps, &[])?;
        params.push(("limit", "1".to_owned()));
        parse_feed(&self.get(&params).await?)?;
        Ok(caps)
    }
}

#[cfg(test)]
mod tests;
