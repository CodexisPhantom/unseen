//! Tags (docs/04-database.md). Labels follow Radarr's rule: lower case
//! letters, digits and `-`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::entities::tags;
use db::tags::TagError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{LibraryEdit, LibraryRead, Require};
use crate::audit::{Actor, ClientIp, Event, record};
use crate::error::Problem;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    id: i32,
    label: String,
}

impl From<tags::Model> for Tag {
    fn from(model: tags::Model) -> Self {
        Self {
            id: model.id,
            label: model.label,
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagBody {
    /// Lower case letters, digits and `-`. Upper case is lowered.
    label: String,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "tags");
    Problem::internal()
}

fn tag_error(error: TagError) -> Problem {
    match error {
        TagError::NotFound => Problem::not_found(),
        TagError::LabelTaken => Problem::conflict("label_taken", "A tag has this label"),
        TagError::InUse => Problem::conflict(
            "tag_in_use",
            "A movie carries this tag; remove it from the movie first",
        ),
        TagError::Db(error) => failed(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabelError {
    Empty,
    TooLong,
    InvalidChars,
}

impl LabelError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Empty | Self::TooLong => "must have 1 to 64 characters",
            Self::InvalidChars => "may hold only letters, digits and `-`",
        }
    }
}

/// Normalizes and validates a tag label: trimmed, lowercased, 1..=64 ASCII
/// lowercase letters, digits or `-`.
pub(crate) fn validate_label(text: &str) -> Result<String, LabelError> {
    let label = text.trim().to_lowercase();
    if label.is_empty() {
        return Err(LabelError::Empty);
    }
    if label.len() > 64 {
        return Err(LabelError::TooLong);
    }
    if !label
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(LabelError::InvalidChars);
    }
    Ok(label)
}

fn label(text: &str) -> Result<String, Problem> {
    validate_label(text).map_err(|error| Problem::validation("label", error.message()))
}

/// Every tag, ordered by label.
#[utoipa::path(
    get,
    path = "/api/v1/tags",
    tag = "tags",
    responses(
        (status = 200, description = "The tags", body = Vec<Tag>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_tags(
    _: Require<LibraryRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Tag>>, Problem> {
    let tags = db::tags::list(&state.db).await.map_err(failed)?;
    Ok(Json(tags.into_iter().map(Tag::from).collect()))
}

/// Creates a tag.
#[utoipa::path(
    post,
    path = "/api/v1/tags",
    tag = "tags",
    request_body = TagBody,
    responses(
        (status = 201, description = "The new tag", body = Tag),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The label is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_tag(
    caller: Require<LibraryEdit>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<TagBody>,
) -> Result<(StatusCode, Json<Tag>), Problem> {
    let created = db::tags::create(&state.db, label(&body.label)?)
        .await
        .map_err(tag_error)?;
    let event = Event::new("tag.created")
        .target("tag", created.id)
        .data(json!({ "label": created.label }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// Renames a tag.
#[utoipa::path(
    patch,
    path = "/api/v1/tags/{id}",
    tag = "tags",
    params(("id" = i32, Path, description = "Tag id")),
    request_body = TagBody,
    responses(
        (status = 200, description = "The renamed tag", body = Tag),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such tag", body = Problem),
        (status = 409, description = "The label is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_tag(
    caller: Require<LibraryEdit>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<TagBody>,
) -> Result<Json<Tag>, Problem> {
    let new = label(&body.label)?;
    let before = db::tags::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    let renamed = db::tags::rename(&state.db, id, new)
        .await
        .map_err(tag_error)?;
    let event = Event::new("tag.renamed")
        .target("tag", id)
        .data(json!({ "old": before.label, "new": renamed.label }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(renamed.into()))
}

/// Deletes a tag that no movie carries.
#[utoipa::path(
    delete,
    path = "/api/v1/tags/{id}",
    tag = "tags",
    params(("id" = i32, Path, description = "Tag id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such tag", body = Problem),
        (status = 409, description = "A movie carries the tag", body = Problem),
    )
)]
pub async fn delete_tag(
    caller: Require<LibraryEdit>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    let before = db::tags::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    db::tags::delete(&state.db, id).await.map_err(tag_error)?;
    let event = Event::new("tag.deleted")
        .target("tag", id)
        .data(json!({ "label": before.label }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}
