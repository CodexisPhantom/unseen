//! API keys: a secret that stands for some of the permissions of its owner
//! (docs/15-permissions.md, "API keys").

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use unseen_core::permission::Permission;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Caller, Require};
use crate::audit::{Actor, ClientIp, Event, record};
use crate::auth::hex;
use crate::error::Problem;
use crate::me::PermissionName;
use crate::roles::{escalation, known, parse_permissions, valid_name};

/// Characters of the key kept for display.
const PREFIX_LEN: usize = 8;

/// The SHA-256 of a key, as stored.
pub(crate) fn hash(key: &str) -> String {
    // API keys are random high-entropy tokens, not user passwords, so SHA-256 is correct.
    // API keys are 128-bit random tokens, so a fast digest is the safe choice and no password
    // KDF applies.
    // lgtm[rust/insufficient-password-hash,rust/weak-sensitive-data-hashing]
    hex(&Sha256::digest(key.as_bytes()))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    id: i32,
    name: String,
    /// The first characters of the key. The key itself is shown once.
    prefix: String,
    /// The permissions chosen at creation. The key never exceeds what its
    /// owner holds now.
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<&'static str>,
    user_id: String,
    user_name: String,
    created_at: String,
    last_used_at: Option<String>,
}

impl From<db::api_keys::ApiKey> for ApiKey {
    fn from(key: db::api_keys::ApiKey) -> Self {
        Self {
            permissions: known(&key.permissions).map(Permission::as_str).collect(),
            id: key.id,
            name: key.name,
            prefix: key.prefix,
            user_id: key.user_id,
            user_name: key.user_name,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedApiKey {
    /// The key. This is the only response that carries it.
    key: String,
    api_key: ApiKey,
}

#[derive(Deserialize, ToSchema)]
pub struct NewApiKey {
    name: String,
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<String>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "api keys");
    Problem::internal()
}

/// The keys of the signed-in user.
#[utoipa::path(
    get,
    path = "/api/v1/me/api-keys",
    tag = "api-keys",
    responses(
        (status = 200, description = "The keys of the caller", body = Vec<ApiKey>),
        (status = 401, description = "Not signed in", body = Problem),
    )
)]
pub async fn list_own(
    caller: Caller,
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKey>>, Problem> {
    let keys = db::api_keys::list(&state.db, Some(caller.user_id()))
        .await
        .map_err(failed)?;
    Ok(Json(keys.into_iter().map(ApiKey::from).collect()))
}

/// Creates a key with some of the permissions the caller holds. The key is
/// in this response only. A request made with an API key cannot create one.
#[utoipa::path(
    post,
    path = "/api/v1/me/api-keys",
    tag = "api-keys",
    request_body = NewApiKey,
    responses(
        (status = 201, description = "The new key, shown once", body = CreatedApiKey),
        (status = 401, description = "Not signed in", body = Problem),
        (status = 403, description = "A permission the caller lacks, or the request used an API key", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_own(
    caller: Caller,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewApiKey>,
) -> Result<(StatusCode, Json<CreatedApiKey>), Problem> {
    if caller.api_key().is_some() {
        return Err(Problem::refused(
            "session_required",
            "Create API keys signed in, not with an API key",
        ));
    }
    let name = valid_name(&body.name)?;
    let permissions = parse_permissions(&body.permissions)?;
    escalation(&caller, permissions.iter().copied())?;

    let key = hex(&rand::random::<[u8; 16]>());
    let stored = permissions.iter().map(|p| p.as_str().to_owned()).collect();
    let created = db::api_keys::create(
        &state.db,
        caller.user_id(),
        &name,
        &hash(&key),
        &key[..PREFIX_LEN],
        &stored,
    )
    .await
    .map_err(failed)?;
    tracing::info!(user = caller.user_id(), key = created.id, "API key created");
    // The key itself is not recorded, nor its prefix.
    let event = Event::new("api_key.created")
        .target("api_key", created.id)
        .data(json!({ "name": created.name, "permissions": known(&created.permissions).map(Permission::as_str).collect::<Vec<_>>() }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiKey {
            key,
            api_key: created.into(),
        }),
    ))
}

async fn revoke(
    state: &AppState,
    caller: &Caller,
    ip: &ClientIp,
    id: i32,
    owner: Option<&str>,
) -> Result<StatusCode, Problem> {
    let key = db::api_keys::find(&state.db, id)
        .await
        .map_err(failed)?
        .filter(|key| owner.is_none_or(|owner| key.user_id == owner))
        .ok_or_else(Problem::not_found)?;
    db::api_keys::revoke(&state.db, key.id)
        .await
        .map_err(failed)?;
    tracing::info!(key = key.id, "API key revoked");
    state.events.publish(crate::events::AppEvent {
        kind: "api_key.revoked".to_owned(),
        data: serde_json::Value::Null,
        permission: None,
        library_id: None,
        target_user: None,
        target_api_key: Some(key.id),
    });
    let event = Event::new("api_key.revoked")
        .target("api_key", key.id)
        .data(json!({ "name": key.name, "userId": key.user_id }));
    record(state, &Actor::of(state, caller).await, ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Revokes a key of the signed-in user. A key of another user is not found.
#[utoipa::path(
    delete,
    path = "/api/v1/me/api-keys/{id}",
    tag = "api-keys",
    params(("id" = i32, Path, description = "Key id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 401, description = "Not signed in", body = Problem),
        (status = 404, description = "No such key", body = Problem),
    )
)]
pub async fn revoke_own(
    caller: Caller,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    revoke(&state, &caller, &ip, id, Some(caller.user_id())).await
}

/// The keys of every user.
#[utoipa::path(
    get,
    path = "/api/v1/api-keys",
    tag = "api-keys",
    responses(
        (status = 200, description = "Every key", body = Vec<ApiKey>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_all(
    _: Require<crate::access::ApikeysManage>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKey>>, Problem> {
    let keys = db::api_keys::list(&state.db, None).await.map_err(failed)?;
    Ok(Json(keys.into_iter().map(ApiKey::from).collect()))
}

/// Revokes a key of any user.
#[utoipa::path(
    delete,
    path = "/api/v1/api-keys/{id}",
    tag = "api-keys",
    params(("id" = i32, Path, description = "Key id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such key", body = Problem),
    )
)]
pub async fn revoke_any(
    caller: Require<crate::access::ApikeysManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    revoke(&state, &caller, &ip, id, None).await
}
