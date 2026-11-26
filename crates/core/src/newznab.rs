//! The Newznab client (docs/08-integrations.md): `t=caps`, movie search,
//! text search and the latest-releases RSS feed for Usenet indexers.
//!
//! Use only the search modes and parameters that `caps` advertises. A movie
//! is searched by IMDb or TMDB id when the indexer supports one of them,
//! otherwise by text. The API key travels in the `apikey` query parameter, so
//! no error text may carry the request URL.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::RETRY_AFTER;
use reqwest::{Method, StatusCode};
use roxmltree::{Document, Node};

use crate::http::{OutboundClient, UrlError};

#[derive(Debug, PartialEq, Eq)]
pub enum NewznabError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// The indexer answered, and refused the API key (error codes 100 to 109,
    /// or HTTP 401 and 403).
    ApiKey,
    /// The indexer answered with an `<error>` element.
    Indexer { code: u32, description: String },
    /// HTTP 429, or Newznab error 429 / 500 / 501 (request or download limit).
    /// The caller backs off for `retry_after`, when the indexer said how long.
    RateLimited { retry_after: Option<Duration> },
    /// No answer, or an answer that is not Newznab.
    Unavailable(String),
}

impl fmt::Display for NewznabError {
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

impl std::error::Error for NewznabError {}

impl From<UrlError> for NewznabError {
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

/// What a Newznab indexer says it can do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Caps {
    /// `None` when the mode is missing or `available="no"`.
    pub search: Option<SearchMode>,
    pub movie_search: Option<SearchMode>,
    /// Maximum and default result limits from `<limits max= default=/>`.
    pub limits_max: Option<u32>,
    pub limits_default: Option<u32>,
    /// Advertised Usenet retention in days from `<retention days=/>`.
    pub retention_days: Option<u32>,
    /// Categories and sub-categories in one list.
    pub categories: Vec<Category>,
}

/// One NZB result item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NzbRelease {
    pub title: String,
    pub guid: String,
    /// The NZB file address (`application/x-nzb` enclosure, else untyped
    /// enclosure, else `<link>`).
    pub download_url: String,
    pub size: Option<u64>,
    pub published: Option<DateTime<Utc>>,
    /// Posting date on Usenet (`newznab:attr name="usenetdate"`).
    pub usenet_date: Option<DateTime<Utc>>,
    pub categories: Vec<u32>,
    pub grabs: Option<u32>,
    /// Normalized IMDb id with the `tt` prefix.
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i32>,
    pub files: Option<u32>,
    pub poster: Option<String>,
    pub group: Option<String>,
    /// Parsed from `newznab:attr name="password"`: `true` when non-zero.
    pub password_protected: Option<bool>,
}

impl NzbRelease {
    /// The Usenet posting date when present, else the indexer's publish date.
    pub fn posted_at(&self) -> Option<DateTime<Utc>> {
        self.usenet_date.or(self.published)
    }

    /// Age in whole hours relative to `now`, clamped to 0 for future dates.
    pub fn age_hours(&self, now: DateTime<Utc>) -> Option<u64> {
        let posted = self.posted_at()?;
        let seconds = now.signed_duration_since(posted).num_seconds().max(0);
        Some(u64::try_from(seconds).unwrap_or(0) / 3600)
    }

    /// Age in whole days relative to `now`, clamped to 0 for future dates.
    pub fn age_days(&self, now: DateTime<Utc>) -> Option<u64> {
        Some(self.age_hours(now)? / 24)
    }
}

/// What to look for in a movie search.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MovieQuery {
    pub tmdb_id: Option<i32>,
    /// With or without the `tt` prefix.
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i32>,
    /// Newznab category ids; empty searches every category.
    pub categories: Vec<i32>,
}

pub struct NewznabClient {
    http: OutboundClient,
    base_url: String,
    api_key: String,
}

// No derive: the key must not reach a log line.
impl fmt::Debug for NewznabClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewznabClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

type Params = Vec<(&'static str, String)>;

fn unavailable(error: reqwest::Error) -> NewznabError {
    // `without_url`: the URL carries the API key.
    NewznabError::Unavailable(error.without_url().to_string())
}

fn not_newznab() -> NewznabError {
    NewznabError::Unavailable("the address does not answer like a Newznab indexer".to_owned())
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
fn error_element(root: Node, retry_after: Option<Duration>) -> Option<NewznabError> {
    if root.tag_name().name() != "error" {
        return None;
    }
    let code = root
        .attribute("code")
        .and_then(|code| code.parse().ok())
        .unwrap_or(900);
    Some(match code {
        100..110 => NewznabError::ApiKey,
        429 | 500 | 501 => NewznabError::RateLimited { retry_after },
        _ => NewznabError::Indexer {
            code,
            description: root.attribute("description").unwrap_or_default().to_owned(),
        },
    })
}

fn parse_document(xml: &str) -> Result<Document<'_>, NewznabError> {
    Document::parse(xml).map_err(|_| not_newznab())
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
pub fn parse_caps(xml: &str) -> Result<Caps, NewznabError> {
    let document = parse_document(xml)?;
    let root = document.root_element();
    if let Some(error) = error_element(root, None) {
        return Err(error);
    }
    if root.tag_name().name() != "caps" {
        return Err(not_newznab());
    }
    let searching = root.children().find(|n| n.has_tag_name("searching"));
    let limits = root.children().find(|n| n.has_tag_name("limits"));
    let retention = root.children().find(|n| n.has_tag_name("retention"));
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
        limits_max: limits.and_then(|n| n.attribute("max")?.parse().ok()),
        limits_default: limits.and_then(|n| n.attribute("default")?.parse().ok()),
        retention_days: retention.and_then(|n| n.attribute("days")?.parse().ok()),
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

/// `<newznab:attr name= value=/>` values for one name; the namespace prefix
/// does not matter.
fn attrs<'a>(item: Node<'a, 'a>, name: &'a str) -> impl Iterator<Item = &'a str> {
    item.children()
        .filter(|n| n.has_tag_name("attr") && n.attribute("name") == Some(name))
        .filter_map(|n| n.attribute("value"))
}

fn parse_date(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    DateTime::parse_from_rfc2822(raw)
        .or_else(|_| DateTime::parse_from_rfc3339(raw))
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn parse_imdb_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let digits = trimmed
        .strip_prefix("tt")
        .or_else(|| trimmed.strip_prefix("TT"))
        .unwrap_or(trimmed);
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.bytes().all(|b| b == b'0')
    {
        return None;
    }
    Some(format!("tt{digits}"))
}

fn parse_protected_flag(raw: &str) -> Option<bool> {
    let value = raw.trim();
    if let Ok(number) = value.parse::<u32>() {
        return Some(number > 0);
    }
    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes") {
        return Some(true);
    }
    if value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("no") {
        return Some(false);
    }
    None
}

/// Picks the NZB enclosure (`application/x-nzb`), falling back to an untyped
/// enclosure and ignoring unrelated attachments such as cover images.
fn nzb_enclosure<'a>(item: Node<'a, 'a>) -> Option<Node<'a, 'a>> {
    let mut fallback = None;
    for node in item.children().filter(|n| n.has_tag_name("enclosure")) {
        if node
            .attribute("url")
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            continue;
        }
        match node.attribute("type").map(str::trim) {
            Some(kind) if kind.eq_ignore_ascii_case("application/x-nzb") => return Some(node),
            None | Some("") => {
                fallback.get_or_insert(node);
            }
            Some(_) => {}
        }
    }
    fallback
}

fn release(item: Node) -> Option<NzbRelease> {
    let title = text(item, "title")?;
    let enclosure = nzb_enclosure(item);
    let download_url = enclosure
        .and_then(|n| n.attribute("url"))
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
        .or_else(|| text(item, "link"))?;
    let number = |name: &str| {
        attrs(item, name)
            .next()
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    let text_attr = |name: &str| {
        attrs(item, name)
            .map(|v| v.trim().to_owned())
            .find(|v| !v.is_empty())
    };
    let mut categories: Vec<u32> = attrs(item, "category")
        .filter_map(|v| v.trim().parse().ok())
        .collect();
    if categories.is_empty() {
        categories = item
            .children()
            .filter(|n| n.has_tag_name("category"))
            .filter_map(|n| n.text()?.trim().parse().ok())
            .collect();
    }
    Some(NzbRelease {
        guid: text(item, "guid").unwrap_or_else(|| title.clone()),
        size: text(item, "size")
            .and_then(|size| size.parse().ok())
            .or_else(|| number("size"))
            .or_else(|| enclosure.and_then(|n| n.attribute("length")?.trim().parse().ok())),
        published: text(item, "pubDate").and_then(|date| parse_date(&date)),
        usenet_date: attrs(item, "usenetdate").next().and_then(parse_date),
        grabs: number("grabs").and_then(|n| u32::try_from(n).ok()),
        imdb_id: attrs(item, "imdb")
            .chain(attrs(item, "imdbid"))
            .find_map(parse_imdb_id),
        tmdb_id: attrs(item, "tmdbid")
            .chain(attrs(item, "tmdb"))
            .find_map(|v| v.trim().parse::<i32>().ok().filter(|id| *id > 0)),
        files: number("files").and_then(|n| u32::try_from(n).ok()),
        poster: text_attr("poster"),
        group: text_attr("group"),
        password_protected: attrs(item, "password")
            .next()
            .and_then(parse_protected_flag),
        categories,
        download_url,
        title,
    })
}

/// Parses a search or RSS feed answer. An item without a title or without any
/// NZB download address is skipped, because it cannot be grabbed.
pub fn parse_feed(xml: &str) -> Result<Vec<NzbRelease>, NewznabError> {
    let document = parse_document(xml)?;
    let root = document.root_element();
    if let Some(error) = error_element(root, None) {
        return Err(error);
    }
    if root.tag_name().name() != "rss" {
        return Err(not_newznab());
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
        .map(|id| id.trim().trim_start_matches("tt"))
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
pub fn movie_params(caps: &Caps, query: &MovieQuery) -> Result<Params, NewznabError> {
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
            return Err(NewznabError::Unavailable(
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
    params.push(("extended", "1".to_owned()));
    params.extend(categories(&query.categories));
    Ok(params)
}

/// The query parameters of a plain text search (`t=search`), from `caps`.
pub fn text_params(caps: &Caps, query: &str, category_ids: &[i32]) -> Result<Params, NewznabError> {
    let text = query.trim();
    if text.is_empty() {
        return Err(NewznabError::Unavailable(
            "the search text must not be empty".to_owned(),
        ));
    }
    let supports_text = caps.search.as_ref().is_some_and(|mode| mode.supports("q"));
    if !supports_text {
        return Err(NewznabError::Unavailable(
            "the indexer does not support text search".to_owned(),
        ));
    }
    let mut params: Params = vec![
        ("t", "search".to_owned()),
        ("q", text.to_owned()),
        ("extended", "1".to_owned()),
    ];
    params.extend(categories(category_ids));
    Ok(params)
}

/// The query parameters of the latest-releases RSS feed: a movie search
/// without a query, or a plain search when the indexer has no movie mode.
pub fn rss_params(caps: &Caps, category_ids: &[i32]) -> Result<Params, NewznabError> {
    let function = match (&caps.movie_search, &caps.search) {
        (Some(_), _) => "movie",
        (None, Some(_)) => "search",
        (None, None) => {
            return Err(NewznabError::Unavailable(
                "the indexer advertises no search".to_owned(),
            ));
        }
    };
    let mut params: Params = vec![("t", function.to_owned()), ("extended", "1".to_owned())];
    params.extend(categories(category_ids));
    Ok(params)
}

impl NewznabClient {
    /// `base_url` is the Newznab API endpoint, for example
    /// `https://api.nzbgeek.info/api`.
    pub fn new(http: OutboundClient, base_url: &str, api_key: &str) -> Self {
        Self {
            http,
            base_url: base_url.to_owned(),
            api_key: api_key.to_owned(),
        }
    }

    /// Replaces the configured key wherever the indexer echoed it back. The key
    /// travels in the query string and indexer text is untrusted, so error text
    /// could otherwise leak the key to a caller or a log line.
    fn scrub_key(&self, error: NewznabError) -> NewznabError {
        if self.api_key.is_empty() {
            return error;
        }
        let redact = |text: String| text.replace(self.api_key.as_str(), "[redacted]");
        match error {
            NewznabError::Indexer { code, description } => NewznabError::Indexer {
                code,
                description: redact(description),
            },
            NewznabError::Unavailable(reason) => NewznabError::Unavailable(redact(reason)),
            other => other,
        }
    }

    async fn get(&self, params: &[(&'static str, String)]) -> Result<String, NewznabError> {
        let response = self
            .http
            .request(Method::GET, &self.base_url)?
            .query(&[("apikey", self.api_key.as_str())])
            .query(params)
            .send()
            .await
            .map_err(|error| self.scrub_key(unavailable(error)))?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok())
            .map(Duration::from_secs);
        let body = response
            .text()
            .await
            .map_err(|error| self.scrub_key(unavailable(error)))?;
        // Indexers put the error element on 200 and on 4xx/5xx alike.
        if let Ok(document) = Document::parse(&body)
            && let Some(error) = error_element(document.root_element(), retry_after)
        {
            return Err(self.scrub_key(error));
        }
        match status {
            StatusCode::TOO_MANY_REQUESTS => Err(NewznabError::RateLimited { retry_after }),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(NewznabError::ApiKey),
            status if status.is_success() => Ok(body),
            status => Err(self.scrub_key(NewznabError::Unavailable(format!(
                "answered HTTP {status}: {}",
                snippet(&body)
            )))),
        }
    }

    pub async fn caps(&self) -> Result<Caps, NewznabError> {
        parse_caps(&self.get(&[("t", "caps".to_owned())]).await?)
            .map_err(|error| self.scrub_key(error))
    }

    pub async fn search_movie(
        &self,
        caps: &Caps,
        query: &MovieQuery,
    ) -> Result<Vec<NzbRelease>, NewznabError> {
        parse_feed(&self.get(&movie_params(caps, query)?).await?)
            .map_err(|error| self.scrub_key(error))
    }

    pub async fn search_text(
        &self,
        caps: &Caps,
        query: &str,
        category_ids: &[i32],
    ) -> Result<Vec<NzbRelease>, NewznabError> {
        parse_feed(&self.get(&text_params(caps, query, category_ids)?).await?)
            .map_err(|error| self.scrub_key(error))
    }

    /// The newest releases, for RSS sync.
    pub async fn rss(
        &self,
        caps: &Caps,
        category_ids: &[i32],
    ) -> Result<Vec<NzbRelease>, NewznabError> {
        parse_feed(&self.get(&rss_params(caps, category_ids)?).await?)
            .map_err(|error| self.scrub_key(error))
    }

    /// Reads `caps` and then asks for the RSS feed with `limit=1`, because
    /// many Newznab indexers answer `caps` without a key but refuse a search.
    /// Returns the caps.
    pub async fn test(&self) -> Result<Caps, NewznabError> {
        let caps = self.caps().await?;
        let mut params = rss_params(&caps, &[])?;
        params.push(("limit", "1".to_owned()));
        parse_feed(&self.get(&params).await?).map_err(|error| self.scrub_key(error))?;
        Ok(caps)
    }
}

#[cfg(test)]
mod tests;
