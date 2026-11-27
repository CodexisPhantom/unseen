//! Size limits per quality (docs/07-release-scoring.md, "Quality definitions
//! (size limits)").

use axum::Json;
use axum::extract::{Path, State};
use db::entities::quality_definitions;
use db::quality_definitions::Values;
use serde::{Deserialize, Deserializer, Serialize};
use unseenarr_core::quality::{Quality, SizeLimits};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{ProfilesManage, ProfilesRead, Require};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QualityDefinition {
    /// Radarr's id of the quality.
    quality_id: i32,
    /// Radarr's name of the quality, for example `Bluray-1080p`.
    name: String,
    /// The name shown to users. Starts equal to `name`.
    title: String,
    /// Megabytes per minute of runtime.
    min_size: f64,
    /// Megabytes per minute. Absent: no preference.
    preferred_size: Option<f64>,
    /// Megabytes per minute. Absent: no limit.
    max_size: Option<f64>,
}

/// A field that is absent keeps the stored value; `null` clears the two
/// optional sizes.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QualityDefinitionPatch {
    title: Option<String>,
    min_size: Option<f64>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<f64>, nullable = true)]
    preferred_size: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<f64>, nullable = true)]
    max_size: Option<Option<f64>>,
}

/// Tells an absent field (`None`) from a `null` one (`Some(None)`).
fn nullable<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(deserializer).map(Some)
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "quality definitions");
    Problem::internal()
}

fn response(model: quality_definitions::Model) -> Option<QualityDefinition> {
    let quality = Quality::from_id(model.quality_id)?;
    Some(QualityDefinition {
        quality_id: model.quality_id,
        name: quality.name().to_owned(),
        title: model.title,
        min_size: model.min_size,
        preferred_size: model.preferred_size,
        max_size: model.max_size,
    })
}

/// Every quality with its size limits, lowest quality first.
#[utoipa::path(
    get,
    path = "/api/v1/quality-definitions",
    tag = "quality-definitions",
    responses(
        (status = 200, description = "The definitions, in the default quality order", body = Vec<QualityDefinition>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_definitions(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<QualityDefinition>>, Problem> {
    let mut rows = db::quality_definitions::list(&state.db)
        .await
        .map_err(failed)?;
    let position = |id: i32| Quality::ALL.iter().position(|quality| quality.id() == id);
    rows.sort_by_key(|row| position(row.quality_id));
    Ok(Json(rows.into_iter().filter_map(response).collect()))
}

/// Changes the limits of one quality.
#[utoipa::path(
    patch,
    path = "/api/v1/quality-definitions/{qualityId}",
    tag = "quality-definitions",
    params(("qualityId" = i32, Path, description = "Radarr's id of the quality")),
    request_body = QualityDefinitionPatch,
    responses(
        (status = 200, description = "The changed definition", body = QualityDefinition),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such quality", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_definition(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(quality_id): Path<i32>,
    Json(body): Json<QualityDefinitionPatch>,
) -> Result<Json<QualityDefinition>, Problem> {
    let current = db::quality_definitions::find(&state.db, quality_id)
        .await
        .map_err(failed)?
        .and_then(response)
        .ok_or_else(Problem::not_found)?;
    let title = match body.title {
        Some(title) if title.trim().is_empty() => {
            return Err(Problem::validation("title", "must not be empty"));
        }
        Some(title) => title.trim().to_owned(),
        None => current.title.clone(),
    };
    let values = Values {
        title,
        min_size: body.min_size.unwrap_or(current.min_size),
        preferred_size: body.preferred_size.unwrap_or(current.preferred_size),
        max_size: body.max_size.unwrap_or(current.max_size),
    };
    let limits = SizeLimits {
        min: values.min_size,
        preferred: values.preferred_size,
        max: values.max_size,
    };
    if let Err(error) = limits.validate() {
        return Err(Problem::validation(error.field, error.message));
    }

    let updated = db::quality_definitions::update(&state.db, quality_id, values)
        .await
        .map_err(failed)?
        .and_then(response)
        .ok_or_else(Problem::not_found)?;
    let changes = Changes::new()
        .field("title", &current.title, &updated.title)
        .field("minSize", current.min_size, updated.min_size)
        .field(
            "preferredSize",
            current.preferred_size,
            updated.preferred_size,
        )
        .field("maxSize", current.max_size, updated.max_size);
    let event = Event::new("quality_definition.updated")
        .target("quality_definition", quality_id)
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(updated))
}
