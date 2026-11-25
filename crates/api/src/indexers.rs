//! Indexers (docs/04-database.md, docs/08-integrations.md). The API key is
//! write-only: no response and no audit entry carries it.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::indexers::{Indexer as Stored, IndexerError, Values};
use db::secret::{Secret, decrypt, encrypt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseen_core::http::check_url_text;
use unseen_core::torznab::{TorznabClient, TorznabError};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{IndexersManage, IndexersRead, Require};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;
use crate::proxy::indexer_client;

/// The only indexer protocol in V1.
const TORZNAB: &str = "torznab";
const DEFAULT_PRIORITY: i32 = 25;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Indexer {
    id: i32,
    name: String,
    /// `torznab`.
    kind: String,
    url: String,
    /// The key itself is never returned.
    has_api_key: bool,
    /// A key is stored but cannot be decrypted: enter it again.
    api_key_must_be_reentered: bool,
    /// Torznab category ids to search, for example `2000` for movies.
    categories: Vec<i32>,
    /// 1 to 50. The lower number wins when two releases tie.
    priority: i32,
    /// Send the traffic through the configured proxy.
    use_proxy: bool,
    enabled: bool,
    /// An indexer without tags serves every movie; one with tags serves the
    /// movies that share a tag (docs/04-database.md).
    tags: Vec<i32>,
    created_at: String,
}

fn default_kind() -> String {
    TORZNAB.to_owned()
}

fn default_priority() -> i32 {
    DEFAULT_PRIORITY
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewIndexer {
    name: String,
    #[serde(default = "default_kind")]
    kind: String,
    url: String,
    /// Stored encrypted and never returned.
    api_key: String,
    #[serde(default)]
    categories: Vec<i32>,
    #[serde(default = "default_priority")]
    priority: i32,
    #[serde(default = "default_true")]
    use_proxy: bool,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    tags: Vec<i32>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexerPatch {
    name: Option<String>,
    url: Option<String>,
    /// Absent keeps the stored key.
    api_key: Option<String>,
    categories: Option<Vec<i32>>,
    priority: Option<i32>,
    use_proxy: Option<bool>,
    enabled: Option<bool>,
    /// Replaces the whole tag set.
    tags: Option<Vec<i32>>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "indexers");
    Problem::internal()
}

fn indexer_error(error: IndexerError) -> Problem {
    match error {
        IndexerError::NotFound => Problem::not_found(),
        IndexerError::NameTaken => Problem::conflict("name_taken", "An indexer has this name"),
        IndexerError::UnknownTag => Problem::validation("tags", "must be ids of existing tags"),
        IndexerError::Db(error) => failed(error),
    }
}

fn response(state: &AppState, stored: Stored) -> Result<Indexer, Problem> {
    let model = stored.model;
    let readable = decrypt(&state.secrets, &model.api_key).is_ok();
    Ok(Indexer {
        id: model.id,
        name: model.name,
        kind: model.kind,
        url: model.url,
        has_api_key: readable,
        api_key_must_be_reentered: !readable,
        categories: serde_json::from_value(model.categories).map_err(failed)?,
        priority: model.priority,
        use_proxy: model.use_proxy,
        enabled: model.enabled,
        tags: stored.tags,
        created_at: model.created_at,
    })
}

async fn load(state: &AppState, id: i32) -> Result<Stored, Problem> {
    db::indexers::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

/// The rules for a whole indexer; `api_key` is already encrypted.
fn checked(new: NewIndexer, api_key: String) -> Result<Values, Problem> {
    let name = new.name.trim().to_owned();
    if name.is_empty() {
        return Err(Problem::validation("name", "must not be empty"));
    }
    if new.kind != TORZNAB {
        return Err(Problem::validation("kind", "must be `torznab`"));
    }
    let url = new.url.trim().to_owned();
    if let Err(error) = check_url_text(&url) {
        return Err(Problem::validation("url", &error.to_string()));
    }
    if !(1..=50).contains(&new.priority) {
        return Err(Problem::validation("priority", "must be from 1 to 50"));
    }
    if new.categories.iter().any(|id| *id < 0) {
        return Err(Problem::validation("categories", "must not be negative"));
    }
    Ok(Values {
        name,
        kind: new.kind,
        url,
        api_key,
        categories: json!(new.categories),
        priority: new.priority,
        use_proxy: new.use_proxy,
        enabled: new.enabled,
        tags: new.tags,
    })
}

fn encrypted(state: &AppState, key: &str, field: &str) -> Result<String, Problem> {
    let key = key.trim();
    if key.is_empty() {
        return Err(Problem::validation(field, "must not be empty"));
    }
    Ok(encrypt(&state.secrets, &Secret::new(key)))
}

/// Every indexer, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/indexers",
    tag = "indexers",
    responses(
        (status = 200, description = "The indexers", body = Vec<Indexer>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_indexers(
    _: Require<IndexersRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Indexer>>, Problem> {
    let stored = db::indexers::list(&state.db).await.map_err(failed)?;
    stored
        .into_iter()
        .map(|stored| response(&state, stored))
        .collect::<Result<_, _>>()
        .map(Json)
}

/// One indexer.
#[utoipa::path(
    get,
    path = "/api/v1/indexers/{id}",
    tag = "indexers",
    params(("id" = i32, Path, description = "Indexer id")),
    responses(
        (status = 200, description = "The indexer", body = Indexer),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such indexer", body = Problem),
    )
)]
pub async fn get_indexer(
    _: Require<IndexersRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Indexer>, Problem> {
    response(&state, load(&state, id).await?).map(Json)
}

/// Adds an indexer. The key is stored encrypted.
#[utoipa::path(
    post,
    path = "/api/v1/indexers",
    tag = "indexers",
    request_body = NewIndexer,
    responses(
        (status = 201, description = "The new indexer", body = Indexer),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_indexer(
    caller: Require<IndexersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewIndexer>,
) -> Result<(StatusCode, Json<Indexer>), Problem> {
    let api_key = encrypted(&state, &body.api_key, "apiKey")?;
    let created = db::indexers::create(&state.db, checked(body, api_key)?)
        .await
        .map_err(indexer_error)?;
    let event = Event::new("indexer.created")
        .target("indexer", created.model.id)
        .data(json!({ "name": created.model.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(response(&state, created)?)))
}

/// Changes an indexer. A request without `apiKey` keeps the stored key.
#[utoipa::path(
    patch,
    path = "/api/v1/indexers/{id}",
    tag = "indexers",
    params(("id" = i32, Path, description = "Indexer id")),
    request_body = IndexerPatch,
    responses(
        (status = 200, description = "The changed indexer", body = Indexer),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such indexer", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_indexer(
    caller: Require<IndexersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<IndexerPatch>,
) -> Result<Json<Indexer>, Problem> {
    let current = load(&state, id).await?;
    let before = response(&state, current.clone())?;
    let key_changed = body.api_key.is_some();
    let api_key = match &body.api_key {
        Some(key) => encrypted(&state, key, "apiKey")?,
        None => current.model.api_key,
    };
    let merged = NewIndexer {
        name: body.name.unwrap_or_else(|| before.name.clone()),
        kind: before.kind.clone(),
        url: body.url.unwrap_or_else(|| before.url.clone()),
        api_key: String::new(),
        categories: body.categories.unwrap_or_else(|| before.categories.clone()),
        priority: body.priority.unwrap_or(before.priority),
        use_proxy: body.use_proxy.unwrap_or(before.use_proxy),
        enabled: body.enabled.unwrap_or(before.enabled),
        tags: body.tags.unwrap_or_else(|| before.tags.clone()),
    };
    let updated = db::indexers::update(&state.db, id, checked(merged, api_key)?)
        .await
        .map_err(indexer_error)?;
    let after = response(&state, updated)?;
    let mut changes = Changes::new()
        .field("name", &before.name, &after.name)
        .field("url", &before.url, &after.url)
        .field("categories", &before.categories, &after.categories)
        .field("priority", before.priority, after.priority)
        .field("useProxy", before.use_proxy, after.use_proxy)
        .field("enabled", before.enabled, after.enabled)
        .field("tags", &before.tags, &after.tags);
    if key_changed {
        changes = changes.secret("apiKey");
    }
    let event = Event::new("indexer.updated")
        .target("indexer", id)
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

/// Deletes an indexer.
#[utoipa::path(
    delete,
    path = "/api/v1/indexers/{id}",
    tag = "indexers",
    params(("id" = i32, Path, description = "Indexer id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such indexer", body = Problem),
    )
)]
pub async fn delete_indexer(
    caller: Require<IndexersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    let current = load(&state, id).await?;
    db::indexers::delete(&state.db, id)
        .await
        .map_err(indexer_error)?;
    let event = Event::new("indexer.deleted")
        .target("indexer", id)
        .data(json!({ "name": current.model.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// What the test learned about the indexer.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexerTestResult {
    /// The indexer advertises movie search.
    movie_search: bool,
    /// The indexer advertises plain text search.
    text_search: bool,
    /// Movie search accepts an IMDb id.
    imdb_id: bool,
    /// Movie search accepts a TMDB id.
    tmdb_id: bool,
    categories: Vec<IndexerTestCategory>,
}

#[derive(Serialize, ToSchema)]
pub struct IndexerTestCategory {
    id: u32,
    name: String,
}

fn test_error(error: TorznabError) -> Problem {
    let (code, title) = match &error {
        TorznabError::ApiKey => (
            "indexer_api_key_rejected",
            "The indexer refused the API key",
        ),
        TorznabError::RateLimited { .. } => (
            "indexer_rate_limited",
            "The indexer is rate limiting this server",
        ),
        TorznabError::Url(_) | TorznabError::Indexer { .. } | TorznabError::Unavailable(_) => {
            ("indexer_unavailable", "The indexer is not available")
        }
    };
    tracing::warn!(%error, "indexer test");
    let mut problem = Problem::new(StatusCode::BAD_GATEWAY, code, title);
    problem.detail = Some(error.to_string());
    problem
}

/// Reads the capabilities of the saved indexer and asks it for one item, so
/// a wrong API key is found even when `caps` is open.
#[utoipa::path(
    post,
    path = "/api/v1/indexers/{id}/test",
    tag = "indexers",
    params(("id" = i32, Path, description = "Indexer id")),
    responses(
        (status = 200, description = "The indexer answers", body = IndexerTestResult),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such indexer", body = Problem),
        (status = 409, description = "The stored API key cannot be read", body = Problem),
        (status = 502, description = "The indexer refused the key or did not answer", body = Problem),
    )
)]
pub async fn test_indexer(
    _: Require<IndexersManage>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<IndexerTestResult>, Problem> {
    let model = load(&state, id).await?.model;
    let key = decrypt(&state.secrets, &model.api_key).map_err(|_| {
        Problem::conflict(
            "indexer_api_key_unreadable",
            "The stored API key cannot be read; enter it again",
        )
    })?;
    let http = indexer_client(&state, model.use_proxy).await?;
    let client = TorznabClient::new(http, &model.url, key.expose());
    let caps = client.test().await.map_err(test_error)?;
    let movie = caps.movie_search.as_ref();
    Ok(Json(IndexerTestResult {
        movie_search: movie.is_some(),
        text_search: caps.search.is_some(),
        imdb_id: movie.is_some_and(|mode| mode.supports("imdbid")),
        tmdb_id: movie.is_some_and(|mode| mode.supports("tmdbid")),
        categories: caps
            .categories
            .into_iter()
            .map(|c| IndexerTestCategory {
                id: c.id,
                name: c.name,
            })
            .collect(),
    }))
}
