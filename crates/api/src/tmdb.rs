//! The TMDB key and language settings, and the client the routes build from
//! them (docs/10-configuration.md, docs/08-integrations.md).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use db::settings::{SecretSetting, get_secret, get_string, set_secret, set_string};
use serde::{Deserialize, Serialize};
use unseenarr_core::metadata::{MetadataError, MetadataProvider};
use unseenarr_core::tmdb::{DEFAULT_LANGUAGE, TmdbClient, is_valid_language};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Require, SettingsManage, SettingsRead};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

const KEY: &str = "tmdb.api_key";
const LANGUAGE: &str = "tmdb.language";

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "tmdb settings");
    Problem::internal()
}

/// 409: the server is not set up for this request yet, and the Owner can fix
/// it in settings.
fn not_configured(detail: &str) -> Problem {
    let mut problem = Problem::new(
        StatusCode::CONFLICT,
        "tmdb_not_configured",
        "TMDB is not configured",
    );
    problem.detail = Some(detail.to_owned());
    problem
}

/// The problem for a failed provider call. `NotFound` is the caller's to
/// handle; here it is a plain 404.
pub fn provider_error(error: MetadataError) -> Problem {
    let (code, title) = match &error {
        MetadataError::NotFound => return Problem::not_found(),
        MetadataError::Unauthorized => ("tmdb_key_rejected", "TMDB refused the API key"),
        MetadataError::RateLimited => ("tmdb_rate_limited", "TMDB is rate limiting requests"),
        MetadataError::Unavailable(_) => ("tmdb_unavailable", "TMDB is not available"),
    };
    tracing::warn!(%error, "tmdb");
    Problem::new(StatusCode::BAD_GATEWAY, code, title)
}

async fn language(state: &AppState) -> Result<String, Problem> {
    if let Some(language) = state.settings_overrides.get(LANGUAGE) {
        return Ok(language.expose().to_owned());
    }
    Ok(get_string(&state.db, LANGUAGE)
        .await
        .map_err(failed)?
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned()))
}

/// The key from the environment, else the stored one.
async fn stored_key(state: &AppState) -> Result<Option<SecretSetting>, Problem> {
    if let Some(key) = state.settings_overrides.get(KEY) {
        return Ok(Some(SecretSetting::Set(key.clone())));
    }
    get_secret(&state.db, &state.secrets, KEY)
        .await
        .map_err(failed)
}

/// The provider for the stored key and language, or `tmdb_not_configured`.
pub async fn client(state: &AppState) -> Result<Box<dyn MetadataProvider>, Problem> {
    let key = match stored_key(state).await? {
        Some(SecretSetting::Set(key)) => key,
        Some(SecretSetting::MustReenter) => {
            return Err(not_configured(
                "The stored TMDB API key cannot be read. Enter it again in settings.",
            ));
        }
        None => {
            return Err(not_configured(
                "Set the TMDB API key in settings before searching.",
            ));
        }
    };
    Ok(Box::new(TmdbClient::new(
        state.http.clone(),
        &state.tmdb_base_url,
        key.expose(),
        &language(state).await?,
    )))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TmdbSettings {
    /// The key itself is never returned.
    has_api_key: bool,
    /// A key is stored but cannot be decrypted: enter it again.
    api_key_must_be_reentered: bool,
    /// For example `en-US`.
    language: String,
    /// Fields set by an environment variable (`UNSEENARR_SETTING_...`). They are
    /// read-only: a change is refused with 409 `setting_overridden`.
    overridden: Vec<&'static str>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TmdbSettingsPatch {
    /// A v3 API key or a v4 read access token. Absent keeps the stored key.
    api_key: Option<String>,
    /// For example `en-US`. Absent keeps the stored language.
    language: Option<String>,
}

async fn current(state: &AppState) -> Result<TmdbSettings, Problem> {
    let key = stored_key(state).await?;
    let overridden = [("apiKey", KEY), ("language", LANGUAGE)]
        .into_iter()
        .filter(|(_, key)| state.settings_overrides.contains(key))
        .map(|(field, _)| field)
        .collect();
    Ok(TmdbSettings {
        has_api_key: matches!(key, Some(SecretSetting::Set(_))),
        api_key_must_be_reentered: matches!(key, Some(SecretSetting::MustReenter)),
        language: language(state).await?,
        overridden,
    })
}

/// 409 for a change to a setting that the environment sets.
fn overridden(key: &str) -> Problem {
    let mut problem = Problem::conflict("setting_overridden", "The environment sets this setting");
    problem.detail = Some(format!(
        "{key} is set by {} and cannot be changed here. Remove the variable to edit it.",
        db::settings::Overrides::var_name(key)
    ));
    problem
}

/// The TMDB settings, without the key.
#[utoipa::path(
    get,
    path = "/api/v1/settings/tmdb",
    tag = "settings",
    responses(
        (status = 200, description = "The settings", body = TmdbSettings),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn get_settings(
    _: Require<SettingsRead>,
    State(state): State<AppState>,
) -> Result<Json<TmdbSettings>, Problem> {
    current(&state).await.map(Json)
}

/// Changes the TMDB key and language. The key is stored encrypted.
#[utoipa::path(
    put,
    path = "/api/v1/settings/tmdb",
    tag = "settings",
    request_body = TmdbSettingsPatch,
    responses(
        (status = 200, description = "The settings after the change", body = TmdbSettings),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_settings(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<TmdbSettingsPatch>,
) -> Result<Json<TmdbSettings>, Problem> {
    let api_key = body.api_key.map(|key| key.trim().to_owned());
    if api_key.as_ref().is_some_and(String::is_empty) {
        return Err(Problem::validation("apiKey", "must not be empty"));
    }
    if let Some(language) = &body.language
        && !is_valid_language(language)
    {
        return Err(Problem::validation(
            "language",
            "must look like `en` or `en-US`",
        ));
    }

    if api_key.is_some() && state.settings_overrides.contains(KEY) {
        return Err(overridden(KEY));
    }
    if body.language.is_some() && state.settings_overrides.contains(LANGUAGE) {
        return Err(overridden(LANGUAGE));
    }

    let before = current(&state).await?;
    if let Some(key) = &api_key {
        set_secret(
            &state.db,
            &state.secrets,
            KEY,
            &db::secret::Secret::new(key.as_str()),
        )
        .await
        .map_err(failed)?;
    }
    if let Some(language) = &body.language {
        set_string(&state.db, LANGUAGE, language)
            .await
            .map_err(failed)?;
    }
    let after = current(&state).await?;

    let mut changes = Changes::new().field("language", &before.language, &after.language);
    if api_key.is_some() {
        changes = changes.secret("apiKey");
    }
    let event = Event::new("setting.updated")
        .target("setting", "tmdb")
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}
