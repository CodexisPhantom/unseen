//! Download clients (docs/04-database.md, docs/08-integrations.md). The
//! password and the API key are write-only: no response and no audit entry
//! carries them.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::download_clients::{Client as Stored, ClientError, Values};
use db::secret::{Secret, decrypt, encrypt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseen_core::http::check_url_text;
use unseen_core::qbittorrent::{QbAuth, QbClient, QbError};
use unseen_core::sabnzbd::{SabClient, SabError};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{ClientsManage, ClientsRead, Require};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

/// The download clients in V1.
const QBITTORRENT: &str = "qbittorrent";
const SABNZBD: &str = "sabnzbd";
/// The category Radarr uses, so an existing qBittorrent setup keeps working.
const DEFAULT_CATEGORY: &str = "radarr";

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClient {
    id: i32,
    name: String,
    /// `qbittorrent` or `sabnzbd`.
    kind: String,
    /// The address, for example `http://qbittorrent:8080` or `http://sabnzbd:8080`.
    url: String,
    username: Option<String>,
    /// The password itself is never returned.
    has_password: bool,
    /// A password is stored but cannot be decrypted: enter it again.
    password_must_be_reentered: bool,
    /// The API key (qBittorrent 5.2 or later) is never returned. When one is
    /// set, Unseen uses it and ignores the username and password. SABnzbd
    /// requires one.
    has_api_key: bool,
    /// An API key is stored but cannot be decrypted: enter it again. SABnzbd
    /// requires one.
    api_key_must_be_reentered: bool,
    /// The category of the torrents or NZBs Unseen adds.
    category: String,
    /// Remote path mapping: a path as the client sees it ...
    remote_path: Option<String>,
    /// ... and the same folder as Unseen sees it.
    local_path: Option<String>,
    enabled: bool,
    /// A client without tags serves every movie; one with tags serves the
    /// movies that share a tag (docs/04-database.md).
    tags: Vec<i32>,
    created_at: String,
}

fn default_kind() -> String {
    QBITTORRENT.to_owned()
}

fn default_category() -> String {
    DEFAULT_CATEGORY.to_owned()
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewDownloadClient {
    name: String,
    #[serde(default = "default_kind")]
    kind: String,
    url: String,
    username: Option<String>,
    /// Stored encrypted and never returned.
    password: Option<String>,
    /// Stored encrypted and never returned. qBittorrent 5.2 or later;
    /// required for SABnzbd.
    api_key: Option<String>,
    #[serde(default = "default_category")]
    category: String,
    remote_path: Option<String>,
    local_path: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    tags: Vec<i32>,
}

/// An empty string clears `username`, `password`, `apiKey`, `remotePath` and
/// `localPath`. An absent field keeps the stored value.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientPatch {
    name: Option<String>,
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    api_key: Option<String>,
    category: Option<String>,
    remote_path: Option<String>,
    local_path: Option<String>,
    enabled: Option<bool>,
    /// Replaces the whole tag set.
    tags: Option<Vec<i32>>,
}

#[derive(Serialize, ToSchema)]
pub struct TestResult {
    /// The client version, for example `v4.6.2` or `4.3.2`.
    version: String,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "download clients");
    Problem::internal()
}

fn client_error(error: ClientError) -> Problem {
    match error {
        ClientError::NotFound => Problem::not_found(),
        ClientError::NameTaken => {
            Problem::conflict("name_taken", "A download client has this name")
        }
        ClientError::UnknownTag => Problem::validation("tags", "must be ids of existing tags"),
        ClientError::Db(error) => failed(error),
    }
}

fn response(state: &AppState, stored: Stored) -> DownloadClient {
    let model = stored.model;
    let readable = |stored: &Option<String>| {
        stored
            .as_deref()
            .map(|stored| decrypt(&state.secrets, stored).is_ok())
    };
    let (password, api_key) = (readable(&model.password), readable(&model.api_key));
    DownloadClient {
        id: model.id,
        name: model.name,
        kind: model.kind,
        url: model.url,
        username: model.username,
        has_password: password == Some(true),
        password_must_be_reentered: password == Some(false),
        has_api_key: api_key == Some(true),
        api_key_must_be_reentered: api_key == Some(false),
        category: model.category,
        remote_path: model.remote_path,
        local_path: model.local_path,
        enabled: model.enabled,
        tags: stored.tags,
        created_at: model.created_at,
    }
}

async fn load(state: &AppState, id: i32) -> Result<Stored, Problem> {
    db::download_clients::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

/// Trims text and turns an empty result into `None`.
fn text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The rules for a whole client; `password` and `api_key` are already
/// encrypted.
fn checked(
    new: NewDownloadClient,
    password: Option<String>,
    api_key: Option<String>,
) -> Result<Values, Problem> {
    let name = new.name.trim().to_owned();
    if name.is_empty() {
        return Err(Problem::validation("name", "must not be empty"));
    }
    if new.kind != QBITTORRENT && new.kind != SABNZBD {
        return Err(Problem::validation(
            "kind",
            "must be `qbittorrent` or `sabnzbd`",
        ));
    }
    let is_sabnzbd = new.kind == SABNZBD;
    if is_sabnzbd && api_key.is_none() {
        return Err(Problem::validation("apiKey", "is required for SABnzbd"));
    }
    let url = new.url.trim().to_owned();
    if let Err(error) = check_url_text(&url) {
        return Err(Problem::validation("url", &error.to_string()));
    }
    let category = new.category.trim().to_owned();
    if category.is_empty() {
        return Err(Problem::validation("category", "must not be empty"));
    }
    let (remote_path, local_path) = (text(new.remote_path), text(new.local_path));
    if remote_path.is_some() != local_path.is_some() {
        return Err(Problem::validation(
            "remotePath",
            "and localPath must be set together",
        ));
    }
    Ok(Values {
        name,
        kind: new.kind,
        url,
        username: if is_sabnzbd { None } else { text(new.username) },
        password: if is_sabnzbd { None } else { password },
        api_key,
        category,
        remote_path,
        local_path,
        enabled: new.enabled,
        tags: new.tags,
    })
}

fn encrypted(state: &AppState, secret: &str) -> Option<String> {
    let secret = secret.trim();
    (!secret.is_empty()).then(|| encrypt(&state.secrets, &Secret::new(secret)))
}

/// Every download client, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/download-clients",
    tag = "download-clients",
    responses(
        (status = 200, description = "The download clients", body = Vec<DownloadClient>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_clients(
    _: Require<ClientsRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<DownloadClient>>, Problem> {
    let stored = db::download_clients::list(&state.db)
        .await
        .map_err(failed)?;
    Ok(Json(
        stored
            .into_iter()
            .map(|stored| response(&state, stored))
            .collect(),
    ))
}

/// Adds a download client. The password and the API key are stored encrypted.
#[utoipa::path(
    post,
    path = "/api/v1/download-clients",
    tag = "download-clients",
    request_body = NewDownloadClient,
    responses(
        (status = 201, description = "The new download client", body = DownloadClient),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_client(
    caller: Require<ClientsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewDownloadClient>,
) -> Result<(StatusCode, Json<DownloadClient>), Problem> {
    let password = body
        .password
        .as_deref()
        .and_then(|password| encrypted(&state, password));
    let api_key = body
        .api_key
        .as_deref()
        .and_then(|key| encrypted(&state, key));
    let created = db::download_clients::create(&state.db, checked(body, password, api_key)?)
        .await
        .map_err(client_error)?;
    let event = Event::new("download_client.created")
        .target("download_client", created.model.id)
        .data(json!({ "name": created.model.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(response(&state, created))))
}

/// Changes a download client. A request without `password` or `apiKey` keeps
/// the stored value; an empty one removes it.
#[utoipa::path(
    patch,
    path = "/api/v1/download-clients/{id}",
    tag = "download-clients",
    params(("id" = i32, Path, description = "Download client id")),
    request_body = DownloadClientPatch,
    responses(
        (status = 200, description = "The changed download client", body = DownloadClient),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such download client", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_client(
    caller: Require<ClientsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<DownloadClientPatch>,
) -> Result<Json<DownloadClient>, Problem> {
    let current = load(&state, id).await?;
    let before = response(&state, current.clone());
    let password_changed = body.password.is_some();
    let password = match &body.password {
        Some(password) => encrypted(&state, password),
        None => current.model.password.clone(),
    };
    let api_key_changed = body.api_key.is_some();
    let api_key = match &body.api_key {
        Some(key) => encrypted(&state, key),
        None => current.model.api_key.clone(),
    };
    let merged = NewDownloadClient {
        name: body.name.unwrap_or_else(|| before.name.clone()),
        kind: before.kind.clone(),
        url: body.url.unwrap_or_else(|| before.url.clone()),
        username: body.username.or_else(|| before.username.clone()),
        password: None,
        api_key: None,
        category: body.category.unwrap_or_else(|| before.category.clone()),
        remote_path: body.remote_path.or_else(|| before.remote_path.clone()),
        local_path: body.local_path.or_else(|| before.local_path.clone()),
        enabled: body.enabled.unwrap_or(before.enabled),
        tags: body.tags.unwrap_or_else(|| before.tags.clone()),
    };
    let updated = db::download_clients::update(&state.db, id, checked(merged, password, api_key)?)
        .await
        .map_err(client_error)?;
    let after = response(&state, updated);
    let mut changes = Changes::new()
        .field("name", &before.name, &after.name)
        .field("url", &before.url, &after.url)
        .field("username", &before.username, &after.username)
        .field("category", &before.category, &after.category)
        .field("remotePath", &before.remote_path, &after.remote_path)
        .field("localPath", &before.local_path, &after.local_path)
        .field("enabled", before.enabled, after.enabled)
        .field("tags", &before.tags, &after.tags);
    if password_changed {
        changes = changes.secret("password");
    }
    if api_key_changed {
        changes = changes.secret("apiKey");
    }
    let event = Event::new("download_client.updated")
        .target("download_client", id)
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

/// Deletes a download client.
#[utoipa::path(
    delete,
    path = "/api/v1/download-clients/{id}",
    tag = "download-clients",
    params(("id" = i32, Path, description = "Download client id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such download client", body = Problem),
    )
)]
pub async fn delete_client(
    caller: Require<ClientsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    let current = load(&state, id).await?;
    db::download_clients::delete(&state.db, id)
        .await
        .map_err(client_error)?;
    let event = Event::new("download_client.deleted")
        .target("download_client", id)
        .data(json!({ "name": current.model.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) enum ConnectedClient {
    Qbittorrent(QbClient),
    Sabnzbd(SabClient),
}

pub(crate) fn qb_error(error: QbError) -> Problem {
    let (code, title) = match &error {
        QbError::Credentials => (
            "client_credentials_rejected",
            "The download client refused the username or password",
        ),
        QbError::ApiKey => (
            "client_api_key_rejected",
            "The download client refused the API key",
        ),
        QbError::Banned => (
            "client_banned",
            "The download client banned this address after failed logins",
        ),
        QbError::Url(_) | QbError::Unavailable(_) => {
            ("client_unavailable", "The download client is not available")
        }
    };
    tracing::warn!(%error, "download client");
    let mut problem = Problem::new(StatusCode::BAD_GATEWAY, code, title);
    problem.detail = Some(error.to_string());
    problem
}

pub(crate) fn sab_error(error: SabError) -> Problem {
    let (code, title) = match &error {
        SabError::ApiKey => (
            "client_api_key_rejected",
            "The download client refused the API key",
        ),
        SabError::Url(_) | SabError::Unavailable(_) => {
            ("client_unavailable", "The download client is not available")
        }
    };
    tracing::warn!(%error, "download client");
    let mut problem = Problem::new(StatusCode::BAD_GATEWAY, code, title);
    problem.detail = Some(error.to_string());
    problem
}

pub(crate) fn connect(
    state: &AppState,
    model: &db::entities::download_clients::Model,
) -> Result<ConnectedClient, Problem> {
    let unreadable = |code: &str, title: &str| Problem::conflict(code, title);
    if model.kind == SABNZBD {
        let stored = model.api_key.as_deref().ok_or_else(|| {
            unreadable(
                "client_api_key_missing",
                "The SABnzbd client has no API key; enter one",
            )
        })?;
        let key = decrypt(&state.secrets, stored)
            .map_err(|_| {
                unreadable(
                    "client_api_key_unreadable",
                    "The stored API key cannot be read; enter it again",
                )
            })?
            .expose()
            .to_owned();
        return Ok(ConnectedClient::Sabnzbd(SabClient::new(
            state.http.clone(),
            &model.url,
            &key,
        )));
    }
    let auth = match (&model.api_key, &model.password) {
        (Some(stored), _) => QbAuth::ApiKey(
            decrypt(&state.secrets, stored)
                .map_err(|_| {
                    unreadable(
                        "client_api_key_unreadable",
                        "The stored API key cannot be read; enter it again",
                    )
                })?
                .expose()
                .to_owned(),
        ),
        (None, password) => QbAuth::Password {
            username: model.username.clone().unwrap_or_default(),
            password: match password {
                None => String::new(),
                Some(stored) => decrypt(&state.secrets, stored)
                    .map_err(|_| {
                        unreadable(
                            "client_password_unreadable",
                            "The stored password cannot be read; enter it again",
                        )
                    })?
                    .expose()
                    .to_owned(),
            },
        },
    };
    Ok(ConnectedClient::Qbittorrent(QbClient::new(
        state.http.clone(),
        &model.url,
        auth,
    )))
}

/// Authenticates to the saved download client (with its API key when it has
/// one, else with the username and password) and reports its version.
#[utoipa::path(
    post,
    path = "/api/v1/download-clients/{id}/test",
    tag = "download-clients",
    params(("id" = i32, Path, description = "Download client id")),
    responses(
        (status = 200, description = "The login works", body = TestResult),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such download client", body = Problem),
        (status = 409, description = "The stored password or API key cannot be read", body = Problem),
        (status = 502, description = "The client refused the login or did not answer", body = Problem),
    )
)]
pub async fn test_client(
    _: Require<ClientsManage>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<TestResult>, Problem> {
    let model = load(&state, id).await?.model;
    let version = match connect(&state, &model)? {
        ConnectedClient::Sabnzbd(client) => client.test().await.map_err(sab_error)?,
        ConnectedClient::Qbittorrent(client) => client.test().await.map_err(qb_error)?,
    };
    Ok(Json(TestResult { version }))
}
