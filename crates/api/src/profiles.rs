//! Quality profiles (docs/07-release-scoring.md, "Quality profile").

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::entities::quality_profiles;
use db::profiles::{FormatScore, ProfileError, ProfileFormatScore as StoredFormatScore, Values};
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

/// One custom format and the score this profile assigns to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFormatScore {
    custom_format_id: i32,
    name: String,
    score: i32,
}

impl From<StoredFormatScore> for ProfileFormatScore {
    fn from(entry: StoredFormatScore) -> Self {
        Self {
            custom_format_id: entry.custom_format_id,
            name: entry.name,
            score: entry.score,
        }
    }
}

/// A score assignment sent when creating or updating a profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormatScoreInput {
    custom_format_id: i32,
    score: i32,
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
    /// Every custom format, ordered by name, with its score for this profile.
    format_scores: Vec<ProfileFormatScore>,
    created_at: String,
    updated_at: String,
}

fn into_response(
    model: quality_profiles::Model,
    format_scores: Vec<StoredFormatScore>,
) -> Result<QualityProfile, Problem> {
    let items: Vec<Item> = serde_json::from_value(model.items).map_err(failed)?;
    Ok(QualityProfile {
        id: model.id,
        name: model.name,
        upgrade_allowed: model.upgrade_allowed,
        cutoff: model.cutoff_quality_id,
        items: items.into_iter().map(Into::into).collect(),
        language: model.language,
        min_format_score: model.min_format_score,
        cutoff_format_score: model.cutoff_format_score,
        min_upgrade_format_score: model.min_upgrade_format_score,
        format_scores: format_scores.into_iter().map(Into::into).collect(),
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
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
    #[serde(default)]
    format_scores: Vec<FormatScoreInput>,
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
    /// Replaces all custom format scores on this profile when provided.
    format_scores: Option<Vec<FormatScoreInput>>,
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
        ProfileError::UnknownCustomFormat => {
            Problem::validation("formatScores", "must reference existing custom formats")
        }
        ProfileError::Db(error) => failed(error),
    }
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
        min_format_score: profile.min_format_score,
        cutoff_format_score: profile.cutoff_format_score,
        min_upgrade_format_score: profile.min_upgrade_format_score,
    })
    .map_err(|(field, message)| Problem::validation(field, &message))?;
    let mut seen = BTreeSet::new();
    let mut format_scores = Vec::new();
    for entry in profile.format_scores {
        if !seen.insert(entry.custom_format_id) {
            return Err(Problem::validation(
                "formatScores",
                "each custom format may appear at most once",
            ));
        }
        format_scores.push(FormatScore {
            custom_format_id: entry.custom_format_id,
            score: entry.score,
        });
    }
    Ok(Values {
        name: profile.name,
        upgrade_allowed: profile.upgrade_allowed,
        cutoff_quality_id: profile.cutoff,
        items: serde_json::to_value(items).map_err(failed)?,
        language: profile.language,
        min_format_score: profile.min_format_score,
        cutoff_format_score: profile.cutoff_format_score,
        min_upgrade_format_score: profile.min_upgrade_format_score,
        format_scores,
    })
}

async fn load(state: &AppState, id: i32) -> Result<quality_profiles::Model, Problem> {
    db::profiles::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

async fn load_response(
    state: &AppState,
    model: quality_profiles::Model,
) -> Result<QualityProfile, Problem> {
    let scores = db::profiles::format_scores(&state.db, model.id)
        .await
        .map_err(failed)?;
    into_response(model, scores)
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
    let ids: Vec<i32> = profiles.iter().map(|profile| profile.id).collect();
    let mut scores_map = db::profiles::format_scores_for_profiles(&state.db, &ids)
        .await
        .map_err(failed)?;
    profiles
        .into_iter()
        .map(|model| {
            let scores = scores_map.remove(&model.id).unwrap_or_default();
            into_response(model, scores)
        })
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
    let model = load(&state, id).await?;
    load_response(&state, model).await.map(Json)
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
    Ok((
        StatusCode::CREATED,
        Json(load_response(&state, created).await?),
    ))
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
    let model = load(&state, id).await?;
    let before = load_response(&state, model).await?;
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
        format_scores: body.format_scores.unwrap_or_else(|| {
            before
                .format_scores
                .iter()
                .filter(|entry| entry.score != 0)
                .map(|entry| FormatScoreInput {
                    custom_format_id: entry.custom_format_id,
                    score: entry.score,
                })
                .collect()
        }),
    };
    let updated = db::profiles::update(&state.db, id, checked(merged)?)
        .await
        .map_err(profile_error)?;
    let after = load_response(&state, updated).await?;
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
        )
        .field("formatScores", &before.format_scores, &after.format_scores);
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
