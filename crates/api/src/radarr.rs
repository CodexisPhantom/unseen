//! Radarr v3 compatibility API mounted at `/radarr/api/v3`
//! (docs/14-compatibility.md, section D).

use arr_compat::radarr::dto::TagResource;
use arr_compat::radarr::map;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use db::tags::TagError;
use serde_json::json;
use unseen_core::permission::Permission;

use crate::AppState;
use crate::audit::{Actor, ClientIp, Event, record};
use crate::compat::{ApiKeyCaller, CompatError, CompatRouter, ValidationFailure};
use crate::radarr_movies;
use crate::tags::{LabelError, validate_label};

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
            Permission::LibraryRead,
            Method::GET,
            "/movie",
            radarr_movies::list_movies,
        )
        .require(
            Permission::LibraryRead,
            Method::GET,
            "/movie/lookup",
            radarr_movies::lookup_movie,
        )
        .require(
            Permission::LibraryRead,
            Method::GET,
            "/movie/{id}",
            radarr_movies::get_movie,
        )
        .require(Permission::LibraryRead, Method::GET, "/tag", list_tags)
        .require(Permission::LibraryEdit, Method::POST, "/tag", create_tag)
        .require(
            Permission::LibraryEdit,
            Method::PUT,
            "/tag/{id}",
            update_tag,
        )
}

fn failed(error: impl std::fmt::Display) -> CompatError {
    tracing::error!(%error, "radarr compat");
    CompatError::internal()
}

fn validate_tag_label(raw: &str) -> Result<String, CompatError> {
    validate_label(raw).map_err(|error| {
        let code = match error {
            LabelError::Empty => "NotEmptyValidator",
            LabelError::TooLong => "MaximumLengthValidator",
            LabelError::InvalidChars => "RegularExpressionValidator",
        };
        CompatError::validation_errors(vec![
            ValidationFailure::new("label", error.message())
                .attempted_value(raw)
                .error_code(code),
        ])
    })
}

fn tag_error(error: TagError, attempted_label: &str) -> CompatError {
    match error {
        TagError::NotFound => CompatError::not_found(),
        TagError::LabelTaken => CompatError::validation_errors(vec![
            ValidationFailure::new("label", "A tag with this label already exists")
                .attempted_value(attempted_label)
                .error_code("TagExistsValidator"),
        ]),
        TagError::InUse => CompatError::bad_request("Tag is in use"),
        TagError::Db(error) => failed(error),
    }
}

async fn list_tags(State(state): State<AppState>) -> Result<Json<Vec<TagResource>>, CompatError> {
    let tags = db::tags::list(&state.db).await.map_err(failed)?;
    Ok(Json(
        tags.into_iter()
            .map(|tag| map::tag(tag.id, &tag.label))
            .collect(),
    ))
}

async fn create_tag(
    caller: ApiKeyCaller,
    State(state): State<AppState>,
    ip: ClientIp,
    body: Result<Json<TagResource>, JsonRejection>,
) -> Result<(StatusCode, Json<TagResource>), CompatError> {
    let Json(body) = body.map_err(|error| CompatError::bad_request(error.body_text()))?;
    let label = validate_tag_label(&body.label)?;
    let created = db::tags::create(&state.db, label.clone())
        .await
        .map_err(|error| tag_error(error, &label))?;
    let event = Event::new("tag.created")
        .target("tag", created.id)
        .data(json!({ "label": created.label }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((
        StatusCode::CREATED,
        Json(map::tag(created.id, &created.label)),
    ))
}

async fn update_tag(
    caller: ApiKeyCaller,
    State(state): State<AppState>,
    ip: ClientIp,
    path: Result<Path<i32>, PathRejection>,
    body: Result<Json<TagResource>, JsonRejection>,
) -> Result<(StatusCode, Json<TagResource>), CompatError> {
    let Path(id) = path.map_err(|_| CompatError::bad_request("invalid tag id"))?;
    let Json(body) = body.map_err(|error| CompatError::bad_request(error.body_text()))?;
    let label = validate_tag_label(&body.label)?;
    let before = db::tags::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(CompatError::not_found)?;
    let renamed = db::tags::rename(&state.db, id, label.clone())
        .await
        .map_err(|error| tag_error(error, &label))?;
    let event = Event::new("tag.renamed")
        .target("tag", id)
        .data(json!({ "old": before.label, "new": renamed.label }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(map::tag(renamed.id, &renamed.label)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_matches_arr_compat_url_base() {
        assert_eq!(API_BASE, format!("{}/api/v3", arr_compat::radarr::URL_BASE));
    }
}
