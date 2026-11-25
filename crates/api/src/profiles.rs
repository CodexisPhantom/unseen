//! Quality profiles (docs/07-release-scoring.md, "Quality profile").

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::entities::quality_profiles;
use db::profiles::{ProfileError, Values};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseen_core::profile::{self, Item, Profile};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{ProfilesManage, ProfilesRead, Require};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

/// One entry of the ordered list, lowest quality first.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProfileItem {
    #[serde(rename_all = "camelCase")]
    Quality { quality_id: i32, allowed: bool },
    /// Qualities that count as equal. The id is 1000 or more.
    #[serde(rename_all = "camelCase")]
    Group {
        id: i32,
        name: String,
        quality_ids: Vec<i32>,
        allowed: bool,
    },
}

impl From<Item> for ProfileItem {
    fn from(item: Item) -> Self {
        match item {
            Item::Quality {
                quality_id,
                allowed,
            } => Self::Quality {
                quality_id,
                allowed,
            },
            Item::Group {
                id,
                name,
                quality_ids,
                allowed,
            } => Self::Group {
                id,
                name,
                quality_ids,
                allowed,
            },
        }
    }
}

impl From<ProfileItem> for Item {
    fn from(item: ProfileItem) -> Self {
        match item {
            ProfileItem::Quality {
                quality_id,
                allowed,
            } => Self::Quality {
                quality_id,
                allowed,
            },
            ProfileItem::Group {
                id,
                name,
                quality_ids,
                allowed,
            } => Self::Group {
                id,
                name,
                quality_ids,
                allowed,
            },
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QualityProfile {
    id: i32,
    name: String,
    /// A better release replaces an existing file until the cutoff.
    upgrade_allowed: bool,
    /// The id of an allowed quality or group at which upgrades stop.
    cutoff: i32,
    items: Vec<ProfileItem>,
    /// `any`, `original` or a language code.
    language: String,
    min_format_score: i32,
    cutoff_format_score: i32,
    min_upgrade_format_score: i32,
    created_at: String,
    updated_at: String,
}

impl TryFrom<quality_profiles::Model> for QualityProfile {
    type Error = serde_json::Error;

    fn try_from(model: quality_profiles::Model) -> Result<Self, Self::Error> {
        let items: Vec<Item> = serde_json::from_value(model.items)?;
        Ok(Self {
            id: model.id,
            name: model.name,
            upgrade_allowed: model.upgrade_allowed,
            cutoff: model.cutoff_quality_id,
            items: items.into_iter().map(Into::into).collect(),
            language: model.language,
            min_format_score: model.min_format_score,
            cutoff_format_score: model.cutoff_format_score,
            min_upgrade_format_score: model.min_upgrade_format_score,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    profile::LANGUAGE_ANY.to_owned()
}

fn default_min_upgrade() -> i32 {
    1
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewQualityProfile {
    name: String,
    #[serde(default = "default_true")]
    upgrade_allowed: bool,
    cutoff: i32,
    items: Vec<ProfileItem>,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    min_format_score: i32,
    #[serde(default)]
    cutoff_format_score: i32,
    #[serde(default = "default_min_upgrade")]
    min_upgrade_format_score: i32,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QualityProfilePatch {
    name: Option<String>,
    upgrade_allowed: Option<bool>,
    cutoff: Option<i32>,
    items: Option<Vec<ProfileItem>>,
    language: Option<String>,
    min_format_score: Option<i32>,
    cutoff_format_score: Option<i32>,
    min_upgrade_format_score: Option<i32>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "quality profiles");
    Problem::internal()
}

fn profile_error(error: ProfileError) -> Problem {
    match error {
        ProfileError::NotFound => Problem::not_found(),
        ProfileError::NameTaken => {
            Problem::conflict("name_taken", "A quality profile has this name")
        }
        ProfileError::InUse => Problem::conflict(
            "profile_in_use",
            "A movie uses this quality profile; change the movie first",
        ),
        ProfileError::Db(error) => failed(error),
    }
}

fn response(model: quality_profiles::Model) -> Result<QualityProfile, Problem> {
    model.try_into().map_err(failed)
}

/// Checks the rules and turns the profile into what the database stores.
fn checked(mut profile: NewQualityProfile) -> Result<Values, Problem> {
    profile.name = profile.name.trim().to_owned();
    let items: Vec<Item> = profile.items.into_iter().map(Into::into).collect();
    profile::validate(&Profile {
        name: profile.name.clone(),
        items: items.clone(),
        cutoff_quality_id: profile.cutoff,
        language: profile.language.clone(),
        min_upgrade_format_score: profile.min_upgrade_format_score,
    })
    .map_err(|(field, message)| Problem::validation(field, &message))?;
    Ok(Values {
        name: profile.name,
        upgrade_allowed: profile.upgrade_allowed,
        cutoff_quality_id: profile.cutoff,
        items: serde_json::to_value(items).map_err(failed)?,
        language: profile.language,
        min_format_score: profile.min_format_score,
        cutoff_format_score: profile.cutoff_format_score,
        min_upgrade_format_score: profile.min_upgrade_format_score,
    })
}

async fn load(state: &AppState, id: i32) -> Result<quality_profiles::Model, Problem> {
    db::profiles::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

/// Every quality profile, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/quality-profiles",
    tag = "profiles",
    responses(
        (status = 200, description = "The profiles", body = Vec<QualityProfile>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_profiles(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<QualityProfile>>, Problem> {
    let profiles = db::profiles::list(&state.db).await.map_err(failed)?;
    profiles
        .into_iter()
        .map(response)
        .collect::<Result<_, _>>()
        .map(Json)
}

/// One quality profile.
#[utoipa::path(
    get,
    path = "/api/v1/quality-profiles/{id}",
    tag = "profiles",
    params(("id" = i32, Path, description = "Profile id")),
    responses(
        (status = 200, description = "The profile", body = QualityProfile),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such profile", body = Problem),
    )
)]
pub async fn get_profile(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<QualityProfile>, Problem> {
    response(load(&state, id).await?).map(Json)
}

/// Creates a quality profile. The cutoff must be an allowed quality or group
/// of the profile.
#[utoipa::path(
    post,
    path = "/api/v1/quality-profiles",
    tag = "profiles",
    request_body = NewQualityProfile,
    responses(
        (status = 201, description = "The new profile", body = QualityProfile),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_profile(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewQualityProfile>,
) -> Result<(StatusCode, Json<QualityProfile>), Problem> {
    let created = db::profiles::create(&state.db, checked(body)?)
        .await
        .map_err(profile_error)?;
    let event = Event::new("quality_profile.created")
        .target("quality_profile", created.id)
        .data(json!({ "name": created.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(response(created)?)))
}

/// Changes a quality profile. The result must satisfy the same rules as a
/// new profile.
#[utoipa::path(
    patch,
    path = "/api/v1/quality-profiles/{id}",
    tag = "profiles",
    params(("id" = i32, Path, description = "Profile id")),
    request_body = QualityProfilePatch,
    responses(
        (status = 200, description = "The changed profile", body = QualityProfile),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such profile", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_profile(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<QualityProfilePatch>,
) -> Result<Json<QualityProfile>, Problem> {
    let before = response(load(&state, id).await?)?;
    let merged = NewQualityProfile {
        name: body.name.unwrap_or_else(|| before.name.clone()),
        upgrade_allowed: body.upgrade_allowed.unwrap_or(before.upgrade_allowed),
        cutoff: body.cutoff.unwrap_or(before.cutoff),
        items: body.items.unwrap_or_else(|| before.items.clone()),
        language: body.language.unwrap_or_else(|| before.language.clone()),
        min_format_score: body.min_format_score.unwrap_or(before.min_format_score),
        cutoff_format_score: body
            .cutoff_format_score
            .unwrap_or(before.cutoff_format_score),
        min_upgrade_format_score: body
            .min_upgrade_format_score
            .unwrap_or(before.min_upgrade_format_score),
    };
    let updated = db::profiles::update(&state.db, id, checked(merged)?)
        .await
        .map_err(profile_error)?;
    let after = response(updated)?;
    let changed = Changes::new()
        .field("name", &before.name, &after.name)
        .field(
            "upgradeAllowed",
            before.upgrade_allowed,
            after.upgrade_allowed,
        )
        .field("cutoff", before.cutoff, after.cutoff)
        .field("items", &before.items, &after.items)
        .field("language", &before.language, &after.language)
        .field(
            "minFormatScore",
            before.min_format_score,
            after.min_format_score,
        )
        .field(
            "cutoffFormatScore",
            before.cutoff_format_score,
            after.cutoff_format_score,
        )
        .field(
            "minUpgradeFormatScore",
            before.min_upgrade_format_score,
            after.min_upgrade_format_score,
        );
    let event = Event::new("quality_profile.updated")
        .target("quality_profile", id)
        .data(changed.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

/// Deletes a quality profile that no movie uses.
#[utoipa::path(
    delete,
    path = "/api/v1/quality-profiles/{id}",
    tag = "profiles",
    params(("id" = i32, Path, description = "Profile id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such profile", body = Problem),
        (status = 409, description = "A movie uses the profile", body = Problem),
    )
)]
pub async fn delete_profile(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    let current = load(&state, id).await?;
    db::profiles::delete(&state.db, id)
        .await
        .map_err(profile_error)?;
    let event = Event::new("quality_profile.deleted")
        .target("quality_profile", id)
        .data(json!({ "name": current.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}
