//! The proxy for indexer traffic (docs/08-integrations.md). The URL can hold
//! credentials, so it is stored encrypted and never returned: the response
//! carries the address without them.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use db::secret::Secret;
use db::settings::{SecretSetting, get_bool, get_secret, remove, set_bool, set_secret};
use serde::{Deserialize, Serialize};
use unseen_core::http::{OutboundClient, Proxy, check_url_text};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Require, SettingsManage, SettingsRead};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

const URL: &str = "proxy.url";
const BYPASS_LOCAL: &str = "proxy.bypass_local";
/// What the test button asks for when the request names no address.
const TEST_URL: &str = "https://www.example.com/";

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "proxy settings");
    Problem::internal()
}

fn overridden(key: &str) -> Problem {
    let mut problem = Problem::conflict("setting_overridden", "The environment sets this setting");
    problem.detail = Some(format!(
        "{key} is set by {} and cannot be changed here. Remove the variable to edit it.",
        db::settings::Overrides::var_name(key)
    ));
    problem
}

/// The stored or environment URL: `None` when not set.
async fn stored_url(state: &AppState) -> Result<Option<SecretSetting>, Problem> {
    if let Some(url) = state.settings_overrides.get(URL) {
        return Ok(Some(SecretSetting::Set(url.clone())));
    }
    get_secret(&state.db, &state.secrets, URL)
        .await
        .map_err(failed)
}

async fn bypass_local(state: &AppState) -> Result<bool, Problem> {
    if let Some(value) = state.settings_overrides.get(BYPASS_LOCAL) {
        return Ok(!matches!(
            value.expose().to_ascii_lowercase().as_str(),
            "false" | "0" | "no"
        ));
    }
    Ok(get_bool(&state.db, BYPASS_LOCAL)
        .await
        .map_err(failed)?
        .unwrap_or(true))
}

/// The configured proxy, or `None` when there is none.
async fn configured(state: &AppState) -> Result<Option<Proxy>, Problem> {
    let url = match stored_url(state).await? {
        None => return Ok(None),
        Some(SecretSetting::Set(url)) => url,
        Some(SecretSetting::MustReenter) => {
            return Err(Problem::conflict(
                "proxy_unreadable",
                "The stored proxy URL cannot be read; enter it again in settings",
            ));
        }
    };
    let bypass = bypass_local(state).await?;
    Proxy::parse(url.expose(), bypass)
        .map(Some)
        .map_err(|error| {
            let mut problem = Problem::conflict("proxy_invalid", "The proxy URL is not valid");
            problem.detail = Some(error.to_string());
            problem
        })
}

/// The client for one indexer's traffic. An indexer with `use_proxy` off, or
/// no proxy in settings, gets the plain client. A proxy that cannot be used
/// is an error and never a direct connection: the point of the proxy is that
/// the indexer does not see this address.
pub async fn indexer_client(state: &AppState, use_proxy: bool) -> Result<OutboundClient, Problem> {
    if !use_proxy {
        return Ok(state.http.clone());
    }
    match configured(state).await? {
        None => Ok(state.http.clone()),
        Some(proxy) => OutboundClient::with_proxy(&proxy).map_err(failed),
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    /// A proxy is set.
    has_url: bool,
    /// The proxy address without credentials, for example
    /// `socks5h://proxy.example:1080`.
    address: Option<String>,
    /// The address carries a user name or a password. They are never returned.
    has_credentials: bool,
    /// A URL is stored but cannot be decrypted: enter it again.
    url_must_be_reentered: bool,
    /// Requests to local addresses skip the proxy.
    bypass_local: bool,
    /// Fields set by an environment variable (`UNSEEN_SETTING_...`).
    overridden: Vec<&'static str>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettingsPatch {
    /// `http://`, `https://`, `socks5://` or `socks5h://`, with optional
    /// `user:password@`. Absent keeps the stored URL; an empty string removes
    /// the proxy.
    url: Option<String>,
    /// Absent keeps the stored value.
    bypass_local: Option<bool>,
}

async fn current(state: &AppState) -> Result<ProxySettings, Problem> {
    let url = stored_url(state).await?;
    let proxy = match &url {
        Some(SecretSetting::Set(url)) => Proxy::parse(url.expose(), true).ok(),
        _ => None,
    };
    let overridden = [("url", URL), ("bypassLocal", BYPASS_LOCAL)]
        .into_iter()
        .filter(|(_, key)| state.settings_overrides.contains(key))
        .map(|(field, _)| field)
        .collect();
    Ok(ProxySettings {
        has_url: matches!(url, Some(SecretSetting::Set(_))),
        address: proxy.as_ref().map(Proxy::display),
        has_credentials: proxy.as_ref().is_some_and(Proxy::has_credentials),
        url_must_be_reentered: matches!(url, Some(SecretSetting::MustReenter)),
        bypass_local: bypass_local(state).await?,
        overridden,
    })
}

/// The proxy settings, without the credentials.
#[utoipa::path(
    get,
    path = "/api/v1/settings/proxy",
    tag = "settings",
    responses(
        (status = 200, description = "The settings", body = ProxySettings),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn get_settings(
    _: Require<SettingsRead>,
    State(state): State<AppState>,
) -> Result<Json<ProxySettings>, Problem> {
    current(&state).await.map(Json)
}

/// Changes the proxy. The URL is stored encrypted.
#[utoipa::path(
    put,
    path = "/api/v1/settings/proxy",
    tag = "settings",
    request_body = ProxySettingsPatch,
    responses(
        (status = 200, description = "The settings after the change", body = ProxySettings),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The environment sets this setting", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_settings(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<ProxySettingsPatch>,
) -> Result<Json<ProxySettings>, Problem> {
    let url = body.url.as_deref().map(str::trim);
    if let Some(url) = url.filter(|url| !url.is_empty())
        && let Err(error) = Proxy::parse(url, true)
    {
        return Err(Problem::validation("url", &error.to_string()));
    }
    if url.is_some() && state.settings_overrides.contains(URL) {
        return Err(overridden(URL));
    }
    if body.bypass_local.is_some() && state.settings_overrides.contains(BYPASS_LOCAL) {
        return Err(overridden(BYPASS_LOCAL));
    }

    let before = current(&state).await?;
    match url {
        Some("") => remove(&state.db, URL).await.map_err(failed)?,
        Some(url) => set_secret(&state.db, &state.secrets, URL, &Secret::new(url))
            .await
            .map_err(failed)?,
        None => {}
    }
    if let Some(bypass) = body.bypass_local {
        set_bool(&state.db, BYPASS_LOCAL, bypass)
            .await
            .map_err(failed)?;
    }
    let after = current(&state).await?;

    let mut changes = Changes::new().field("bypassLocal", before.bypass_local, after.bypass_local);
    if url.is_some() {
        changes = changes.secret("url");
    }
    let event = Event::new("setting.updated")
        .target("setting", "proxy")
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

#[derive(Deserialize, ToSchema)]
pub struct ProxyTest {
    /// The address to request through the proxy. Absent: a public page.
    url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ProxyTestResult {
    /// The HTTP status the target answered with, through the proxy.
    status: u16,
}

/// Sends one request through the saved proxy, to check that it works.
#[utoipa::path(
    post,
    path = "/api/v1/settings/proxy/test",
    tag = "settings",
    request_body = ProxyTest,
    responses(
        (status = 200, description = "The proxy carried the request", body = ProxyTestResult),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "No proxy is set, or it cannot be used", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
        (status = 502, description = "The request through the proxy failed", body = Problem),
    )
)]
pub async fn test_proxy(
    _: Require<SettingsManage>,
    State(state): State<AppState>,
    Json(body): Json<ProxyTest>,
) -> Result<Json<ProxyTestResult>, Problem> {
    let target = body.url.as_deref().map_or(TEST_URL, str::trim);
    if let Err(error) = check_url_text(target) {
        return Err(Problem::validation("url", &error.to_string()));
    }
    let proxy = configured(&state)
        .await?
        .ok_or_else(|| Problem::conflict("proxy_not_set", "No proxy is set"))?;
    let client = OutboundClient::with_proxy(&proxy).map_err(failed)?;
    let request = client
        .get(target)
        .map_err(|error| Problem::validation("url", &error.to_string()))?;
    match request.send().await {
        Ok(response) => Ok(Json(ProxyTestResult {
            status: response.status().as_u16(),
        })),
        Err(error) => {
            tracing::warn!(%error, "proxy test");
            let mut problem = Problem::new(
                StatusCode::BAD_GATEWAY,
                "proxy_unavailable",
                "The request through the proxy failed",
            );
            problem.detail = Some(error.without_url().to_string());
            Err(problem)
        }
    }
}
