//! Custom formats API (docs/07-release-scoring.md, "Custom formats").

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::custom_formats::{CustomFormatError, Values};
use db::entities::custom_formats;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};
use unseenarr_core::custom_format::{
    self, CustomFormat as CoreCustomFormat, Field as CoreField, Specification as CoreSpecification,
};
use unseenarr_core::release;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{ProfilesManage, ProfilesRead, Require};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

/// One field of a custom format specification. Preserves extra Radarr metadata
/// fields so imported formats round-trip unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SpecificationField {
    pub name: String,
    #[schema(value_type = Object)]
    pub value: Value,
    #[serde(flatten)]
    #[schema(ignore)]
    pub extra: Map<String, Value>,
}

/// One condition of a custom format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomFormatSpecification {
    #[serde(default)]
    pub name: String,
    pub implementation: String,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default, deserialize_with = "deserialize_spec_fields")]
    pub fields: Vec<SpecificationField>,
    #[serde(flatten)]
    #[schema(ignore)]
    pub extra: Map<String, Value>,
}

/// Reads Radarr's field list or the TRaSH guide's field map while preserving
/// Radarr metadata fields on list entries.
fn deserialize_spec_fields<'de, D>(deserializer: D) -> Result<Vec<SpecificationField>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Array(entries) => entries
            .into_iter()
            .map(|entry| serde_json::from_value(entry).map_err(serde::de::Error::custom))
            .collect(),
        Value::Object(entries) => Ok(entries
            .into_iter()
            .map(|(name, value)| SpecificationField {
                name,
                value,
                extra: Map::new(),
            })
            .collect()),
        _ => Err(serde::de::Error::custom(
            "fields must be a list of field objects or a map",
        )),
    }
}

impl From<&CustomFormatSpecification> for CoreSpecification {
    fn from(spec: &CustomFormatSpecification) -> Self {
        Self {
            name: spec.name.clone(),
            implementation: spec.implementation.clone(),
            negate: spec.negate,
            required: spec.required,
            fields: spec
                .fields
                .iter()
                .map(|field| CoreField {
                    name: field.name.clone(),
                    value: field.value.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomFormat {
    id: i32,
    name: String,
    include_custom_format_when_renaming: bool,
    specifications: Vec<CustomFormatSpecification>,
    trash_id: Option<String>,
    trash_synced: bool,
    created_at: String,
    updated_at: String,
}

impl TryFrom<custom_formats::Model> for CustomFormat {
    type Error = serde_json::Error;

    fn try_from(model: custom_formats::Model) -> Result<Self, Self::Error> {
        let specifications: Vec<CustomFormatSpecification> =
            serde_json::from_value(model.specifications)?;
        Ok(Self {
            id: model.id,
            name: model.name,
            include_custom_format_when_renaming: model.include_when_renaming,
            specifications,
            trash_id: model.trash_id,
            trash_synced: model.trash_synced,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }
}

/// Custom format in Radarr's import/export JSON shape (without database ids).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RadarrCustomFormat {
    pub name: String,
    #[serde(default)]
    pub include_custom_format_when_renaming: bool,
    #[serde(default)]
    pub specifications: Vec<CustomFormatSpecification>,
}

impl From<CustomFormat> for RadarrCustomFormat {
    fn from(format: CustomFormat) -> Self {
        Self {
            name: format.name,
            include_custom_format_when_renaming: format.include_custom_format_when_renaming,
            specifications: format.specifications,
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewCustomFormat {
    name: String,
    #[serde(default)]
    include_custom_format_when_renaming: bool,
    #[serde(default)]
    specifications: Vec<CustomFormatSpecification>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomFormatPatch {
    name: Option<String>,
    include_custom_format_when_renaming: Option<bool>,
    specifications: Option<Vec<CustomFormatSpecification>>,
}

/// One Radarr custom format object or an array of them.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum ImportCustomFormatsRequest {
    Many(Vec<RadarrCustomFormat>),
    One(RadarrCustomFormat),
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestCustomFormatsRequest {
    release_name: String,
    #[serde(default)]
    size_bytes: Option<i64>,
    #[serde(default)]
    indexer_flags: Vec<i32>,
    #[serde(default)]
    movie_year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParsedReleaseSummary {
    title: String,
    year: Option<u16>,
    resolution: Option<i32>,
    source: i32,
    quality_modifier: i32,
    release_group: Option<String>,
    edition: Option<String>,
    languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MatchedCustomFormat {
    id: i32,
    name: String,
    include_custom_format_when_renaming: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MatchedFormatScore {
    format_id: i32,
    format_name: String,
    score: i32,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileScoreResult {
    profile_id: i32,
    profile_name: String,
    total_score: i32,
    min_format_score: i32,
    cutoff_format_score: i32,
    format_scores: Vec<MatchedFormatScore>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomFormatTestResult {
    parsed: ParsedReleaseSummary,
    matched_formats: Vec<MatchedCustomFormat>,
    profile_scores: Vec<ProfileScoreResult>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "custom formats");
    Problem::internal()
}

fn format_error(error: CustomFormatError) -> Problem {
    match error {
        CustomFormatError::NotFound => Problem::not_found(),
        CustomFormatError::NameTaken => {
            Problem::conflict("name_taken", "A custom format has this name")
        }
        CustomFormatError::Db(error) => failed(error),
    }
}

fn response(model: custom_formats::Model) -> Result<CustomFormat, Problem> {
    model.try_into().map_err(failed)
}

async fn load(state: &AppState, id: i32) -> Result<custom_formats::Model, Problem> {
    db::custom_formats::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

fn validate_and_prepare(
    name: String,
    include_when_renaming: bool,
    mut specifications: Vec<CustomFormatSpecification>,
    trash_id: Option<String>,
    trash_synced: bool,
) -> Result<Values, Problem> {
    let name = name.trim().to_owned();
    for spec in &mut specifications {
        spec.extra.remove("id");
    }
    let core_format = CoreCustomFormat {
        include_custom_format_when_renaming: include_when_renaming,
        name: name.clone(),
        specifications: specifications.iter().map(CoreSpecification::from).collect(),
    };
    core_format
        .validate()
        .map_err(|err| Problem::validation(&err.field, &err.message))?;
    Ok(Values {
        name,
        include_when_renaming,
        specifications: serde_json::to_value(specifications).map_err(failed)?,
        trash_id,
        trash_synced,
    })
}

/// Every custom format, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/custom-formats",
    tag = "custom-formats",
    responses(
        (status = 200, description = "The custom formats", body = Vec<CustomFormat>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_custom_formats(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<CustomFormat>>, Problem> {
    let rows = db::custom_formats::list(&state.db).await.map_err(failed)?;
    rows.into_iter()
        .map(response)
        .collect::<Result<_, _>>()
        .map(Json)
}

/// One custom format.
#[utoipa::path(
    get,
    path = "/api/v1/custom-formats/{id}",
    tag = "custom-formats",
    params(("id" = i32, Path, description = "Custom format id")),
    responses(
        (status = 200, description = "The custom format", body = CustomFormat),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such custom format", body = Problem),
    )
)]
pub async fn get_custom_format(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<CustomFormat>, Problem> {
    response(load(&state, id).await?).map(Json)
}

/// Creates a custom format.
#[utoipa::path(
    post,
    path = "/api/v1/custom-formats",
    tag = "custom-formats",
    request_body = NewCustomFormat,
    responses(
        (status = 201, description = "The new custom format", body = CustomFormat),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_custom_format(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewCustomFormat>,
) -> Result<(StatusCode, Json<CustomFormat>), Problem> {
    let values = validate_and_prepare(
        body.name,
        body.include_custom_format_when_renaming,
        body.specifications,
        None,
        false,
    )?;
    let created = db::custom_formats::create(&state.db, values)
        .await
        .map_err(format_error)?;
    let event = Event::new("custom_format.created")
        .target("custom_format", created.id)
        .data(json!({ "name": created.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(response(created)?)))
}

/// Updates a custom format.
#[utoipa::path(
    patch,
    path = "/api/v1/custom-formats/{id}",
    tag = "custom-formats",
    params(("id" = i32, Path, description = "Custom format id")),
    request_body = CustomFormatPatch,
    responses(
        (status = 200, description = "The updated custom format", body = CustomFormat),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such custom format", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_custom_format(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<CustomFormatPatch>,
) -> Result<Json<CustomFormat>, Problem> {
    let before = response(load(&state, id).await?)?;
    let values = validate_and_prepare(
        body.name.unwrap_or_else(|| before.name.clone()),
        body.include_custom_format_when_renaming
            .unwrap_or(before.include_custom_format_when_renaming),
        body.specifications
            .unwrap_or_else(|| before.specifications.clone()),
        before.trash_id.clone(),
        before.trash_synced,
    )?;
    let updated = db::custom_formats::update(&state.db, id, values)
        .await
        .map_err(format_error)?;
    let after = response(updated)?;
    let changed = Changes::new()
        .field("name", &before.name, &after.name)
        .field(
            "includeCustomFormatWhenRenaming",
            before.include_custom_format_when_renaming,
            after.include_custom_format_when_renaming,
        )
        .field(
            "specifications",
            &before.specifications,
            &after.specifications,
        );
    let event = Event::new("custom_format.updated")
        .target("custom_format", id)
        .data(changed.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

/// Deletes a custom format and removes its scores from every quality profile.
#[utoipa::path(
    delete,
    path = "/api/v1/custom-formats/{id}",
    tag = "custom-formats",
    params(("id" = i32, Path, description = "Custom format id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such custom format", body = Problem),
    )
)]
pub async fn delete_custom_format(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    let current = load(&state, id).await?;
    db::custom_formats::delete(&state.db, id)
        .await
        .map_err(format_error)?;
    let event = Event::new("custom_format.deleted")
        .target("custom_format", id)
        .data(json!({ "name": current.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Imports one or many custom formats from Radarr's JSON format.
#[utoipa::path(
    post,
    path = "/api/v1/custom-formats/import",
    tag = "custom-formats",
    request_body = ImportCustomFormatsRequest,
    responses(
        (status = 201, description = "The imported custom formats", body = Vec<CustomFormat>),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "A format name is already taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn import_custom_formats(
    caller: Require<ProfilesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Vec<CustomFormat>>), Problem> {
    let raw_items = match body {
        Value::Array(items) => {
            if items.is_empty() {
                return Err(Problem::validation(
                    "body",
                    "must contain at least one format",
                ));
            }
            items
        }
        Value::Object(_) => vec![body],
        _ => {
            return Err(Problem::validation(
                "body",
                "must be a custom format object or an array of them",
            ));
        }
    };

    let is_batch = raw_items.len() > 1;
    let mut prepared = Vec::new();
    let mut seen_names = HashSet::new();
    for (idx, item) in raw_items.into_iter().enumerate() {
        let parsed: RadarrCustomFormat = serde_json::from_value(item).map_err(|err| {
            Problem::validation(&format!("items[{idx}]"), &format!("invalid format: {err}"))
        })?;
        let values = validate_and_prepare(
            parsed.name,
            parsed.include_custom_format_when_renaming,
            parsed.specifications,
            None,
            false,
        )
        .map_err(|mut problem| {
            if is_batch && let Some(first) = problem.errors.first_mut() {
                first.field = format!("items[{idx}].{}", first.field);
            }
            problem
        })?;
        if !seen_names.insert(values.name.clone()) {
            return Err(Problem::conflict(
                "name_taken",
                &format!("Duplicate custom format name `{}` in import", values.name),
            ));
        }
        prepared.push(values);
    }

    let created = db::custom_formats::create_many(&state.db, prepared)
        .await
        .map_err(format_error)?;
    let actor = Actor::of(&state, &caller).await;
    for model in &created {
        let event = Event::new("custom_format.created")
            .target("custom_format", model.id)
            .data(json!({ "name": model.name, "imported": true }));
        record(&state, &actor, &ip, event).await;
    }
    let out = created
        .into_iter()
        .map(response)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((StatusCode::CREATED, Json(out)))
}

/// Exports every custom format in Radarr's JSON format (without ids).
#[utoipa::path(
    get,
    path = "/api/v1/custom-formats/export",
    tag = "custom-formats",
    responses(
        (status = 200, description = "Exported custom formats", body = Vec<RadarrCustomFormat>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn export_custom_formats(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<RadarrCustomFormat>>, Problem> {
    let rows = db::custom_formats::list(&state.db).await.map_err(failed)?;
    let exported = rows
        .into_iter()
        .map(response)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(RadarrCustomFormat::from)
        .collect();
    Ok(Json(exported))
}

/// Exports one custom format in Radarr's JSON format (without id).
#[utoipa::path(
    get,
    path = "/api/v1/custom-formats/{id}/export",
    tag = "custom-formats",
    params(("id" = i32, Path, description = "Custom format id")),
    responses(
        (status = 200, description = "Exported custom format", body = RadarrCustomFormat),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such custom format", body = Problem),
    )
)]
pub async fn export_custom_format(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<RadarrCustomFormat>, Problem> {
    let format = response(load(&state, id).await?)?;
    Ok(Json(RadarrCustomFormat::from(format)))
}

/// Tests a release name against all custom formats and computes the score for
/// every quality profile.
#[utoipa::path(
    post,
    path = "/api/v1/custom-formats/test",
    tag = "custom-formats",
    request_body = TestCustomFormatsRequest,
    responses(
        (status = 200, description = "Matching formats and profile scores", body = CustomFormatTestResult),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn test_custom_formats(
    _: Require<ProfilesRead>,
    State(state): State<AppState>,
    Json(body): Json<TestCustomFormatsRequest>,
) -> Result<Json<CustomFormatTestResult>, Problem> {
    let release_name = body.release_name.trim();
    if release_name.is_empty() {
        return Err(Problem::validation("releaseName", "must not be empty"));
    }
    if let Some(size) = body.size_bytes
        && size < 0
    {
        return Err(Problem::validation("sizeBytes", "must be 0 or greater"));
    }

    let parsed = release::parse(release_name);
    let language_ids: Vec<i32> = parsed
        .languages
        .iter()
        .filter_map(|lang| custom_format::language_id(*lang))
        .collect();
    let release_info = custom_format::release_info(
        release_name,
        &parsed,
        &language_ids,
        body.size_bytes.unwrap_or(0),
        &body.indexer_flags,
        body.movie_year,
    );

    let (formats, profiles, score_rows) = tokio::try_join!(
        db::custom_formats::list(&state.db),
        db::profiles::list(&state.db),
        db::custom_formats::list_profile_scores(&state.db),
    )
    .map_err(failed)?;

    let mut matched_formats = Vec::new();
    for row in formats {
        let specs: Vec<CoreSpecification> =
            serde_json::from_value(row.specifications).map_err(failed)?;
        let core_format = CoreCustomFormat {
            include_custom_format_when_renaming: row.include_when_renaming,
            name: row.name.clone(),
            specifications: specs,
        };
        if core_format.matches(&release_info) {
            matched_formats.push(MatchedCustomFormat {
                id: row.id,
                name: row.name,
                include_custom_format_when_renaming: row.include_when_renaming,
            });
        }
    }

    let score_map: HashMap<(i32, i32), i32> = score_rows
        .into_iter()
        .map(|row| ((row.quality_profile_id, row.custom_format_id), row.score))
        .collect();

    let profile_scores = profiles
        .into_iter()
        .map(|profile| {
            let mut total_score = 0;
            let format_scores = matched_formats
                .iter()
                .map(|format| {
                    let score = score_map
                        .get(&(profile.id, format.id))
                        .copied()
                        .unwrap_or(0);
                    total_score += score;
                    MatchedFormatScore {
                        format_id: format.id,
                        format_name: format.name.clone(),
                        score,
                    }
                })
                .collect();
            ProfileScoreResult {
                profile_id: profile.id,
                profile_name: profile.name,
                total_score,
                min_format_score: profile.min_format_score,
                cutoff_format_score: profile.cutoff_format_score,
                format_scores,
            }
        })
        .collect();

    let parsed_summary = ParsedReleaseSummary {
        title: parsed.title,
        year: parsed.year,
        resolution: parsed.resolution.map(custom_format::resolution_id),
        source: custom_format::source_id(parsed.source),
        quality_modifier: custom_format::modifier_id(parsed.modifier),
        release_group: parsed.release_group,
        edition: parsed.edition,
        languages: parsed
            .languages
            .iter()
            .map(|lang| lang.code().to_owned())
            .collect(),
    };

    Ok(Json(CustomFormatTestResult {
        parsed: parsed_summary,
        matched_formats,
        profile_scores,
    }))
}
