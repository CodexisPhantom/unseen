//! Radarr v3 compatibility API mounted at `/radarr/api/v3`
//! (docs/14-compatibility.md, section D).

use arr_compat::radarr::dto::{QualityProfileResource, RootFolderResource, SystemResource};
use arr_compat::radarr::map::{self, ProfileView, RootFolderView, SystemView};
use axum::Json;
use axum::extract::State;
use axum::http::Method;
use unseen_core::permission::Permission;
use unseen_core::profile::Item;

use crate::AppState;
use crate::compat::{CompatError, CompatRouter};

/// Mount prefix for the Radarr v3 compatibility router.
pub const API_BASE: &str = "/radarr/api/v3";

/// Every Radarr v3 compatibility route is registered here with its required
/// permission. Routes are served case-insensitively with API-key-only auth;
/// anything unimplemented falls back to a logged `501 Not Implemented` JSON
/// response.
#[must_use]
pub fn routes() -> CompatRouter {
    CompatRouter::with_base(API_BASE)
        .require(
            Permission::SettingsRead,
            Method::GET,
            "/system/status",
            system_status,
        )
        .require(
            Permission::ProfilesRead,
            Method::GET,
            "/qualityprofile",
            list_quality_profiles,
        )
        .require(
            Permission::SettingsRead,
            Method::GET,
            "/rootfolder",
            list_root_folders,
        )
}

fn failed(error: impl std::fmt::Display) -> CompatError {
    tracing::error!(%error, "radarr compat");
    CompatError::internal()
}

async fn system_status(State(state): State<AppState>) -> Json<SystemResource> {
    let startup_path = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let app_data = state
        .logs_dir
        .parent()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut resource = map::system(&SystemView {
        start_time: state.started_at.clone(),
        startup_path,
        app_data,
        os_name: std::env::consts::OS.to_owned(),
        os_version: String::new(),
    });
    if !state.base_path.is_empty() {
        resource.url_base = format!("{}{}", state.base_path, arr_compat::radarr::URL_BASE);
    }
    Json(resource)
}

async fn list_quality_profiles(
    State(state): State<AppState>,
) -> Result<Json<Vec<QualityProfileResource>>, CompatError> {
    let profiles = db::profiles::list(&state.db).await.map_err(failed)?;
    let resources = profiles
        .into_iter()
        .map(|model| {
            let items: Vec<Item> = serde_json::from_value(model.items).map_err(failed)?;
            Ok(map::quality_profile(&ProfileView {
                id: model.id,
                name: model.name,
                upgrade_allowed: model.upgrade_allowed,
                cutoff_quality_id: model.cutoff_quality_id,
                items,
                language: model.language,
                min_format_score: model.min_format_score,
                cutoff_format_score: model.cutoff_format_score,
                min_upgrade_format_score: model.min_upgrade_format_score,
            }))
        })
        .collect::<Result<Vec<_>, CompatError>>()?;
    Ok(Json(resources))
}

async fn list_root_folders(
    State(state): State<AppState>,
) -> Result<Json<Vec<RootFolderResource>>, CompatError> {
    let folders = crate::root_folders::load_all(&state)
        .await
        .map_err(failed)?;
    let resources = folders
        .into_iter()
        .map(|folder| {
            map::root_folder(&RootFolderView {
                id: folder.id,
                path: folder.path,
                accessible: folder.accessible,
                free_space: folder
                    .free_space
                    .map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX)),
                unmapped: folder.unmapped_folders,
            })
        })
        .collect();
    Ok(Json(resources))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_matches_arr_compat_url_base() {
        assert_eq!(API_BASE, format!("{}/api/v3", arr_compat::radarr::URL_BASE));
    }
}
