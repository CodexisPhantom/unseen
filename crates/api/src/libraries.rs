//! Libraries: named groups of root folders used to scope access
//! (docs/15-permissions.md, ADR 0012).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::libraries::{LibraryDetail, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseen_core::permission::Permission;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Require, SettingsManage, SettingsRead, UsersManage};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

const NAME_MAX: usize = 64;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    id: i32,
    name: String,
    root_folder_ids: Vec<i32>,
    created_at: String,
    updated_at: String,
}

impl From<LibraryDetail> for Library {
    fn from(detail: LibraryDetail) -> Self {
        Self {
            id: detail.library.id,
            name: detail.library.name,
            root_folder_ids: detail.root_folder_ids,
            created_at: detail.library.created_at,
            updated_at: detail.library.updated_at,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LibraryBody {
    name: String,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "libraries");
    Problem::internal()
}

fn library_error(error: LibraryError) -> Problem {
    match error {
        LibraryError::NotFound => Problem::not_found(),
        LibraryError::NameTaken => {
            Problem::conflict("library_name_taken", "A library has this name")
        }
        LibraryError::InUse => Problem::conflict(
            "library_in_use",
            "A root folder belongs to this library; move or remove the root folders first",
        ),
        LibraryError::Db(error) => failed(error),
    }
}

fn valid_name(raw: &str) -> Result<String, Problem> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(Problem::validation("name", "must not be empty"));
    }
    if name.chars().count() > NAME_MAX {
        return Err(Problem::validation("name", "must be at most 64 characters"));
    }
    Ok(name.to_owned())
}

/// Every library visible to the caller, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/libraries",
    tag = "libraries",
    responses(
        (status = 200, description = "The libraries", body = Vec<Library>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_libraries(
    caller: Require<SettingsRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Library>>, Problem> {
    let rows = db::libraries::list(&state.db).await.map_err(failed)?;
    Ok(Json(
        rows.into_iter()
            .filter(|detail| caller.allows_in_library(Permission::SettingsRead, detail.library.id))
            .map(Library::from)
            .collect(),
    ))
}

/// Libraries available for scoping user roles and direct grants, for callers
/// with `users:manage`.
#[utoipa::path(
    get,
    path = "/api/v1/assignable-libraries",
    tag = "users",
    responses(
        (status = 200, description = "The libraries", body = Vec<Library>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_assignable_libraries(
    caller: Require<UsersManage>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Library>>, Problem> {
    let rows = db::libraries::list(&state.db).await.map_err(failed)?;
    Ok(Json(
        rows.into_iter()
            .filter(|detail| caller.allows_in_library(Permission::UsersManage, detail.library.id))
            .map(Library::from)
            .collect(),
    ))
}

/// Creates a new library. Requires server-wide `settings:manage`.
#[utoipa::path(
    post,
    path = "/api/v1/libraries",
    tag = "libraries",
    request_body = LibraryBody,
    responses(
        (status = 201, description = "The new library", body = Library),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_library(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<LibraryBody>,
) -> Result<(StatusCode, Json<Library>), Problem> {
    if !caller.has_all_libraries(Permission::SettingsManage) {
        return Err(Problem::forbidden(Permission::SettingsManage));
    }
    let name = valid_name(&body.name)?;
    let created = db::libraries::create(&state.db, name)
        .await
        .map_err(library_error)?;
    let event = Event::new("library.created")
        .target("library", created.library.id)
        .data(json!({ "name": created.library.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// Renames a library.
#[utoipa::path(
    patch,
    path = "/api/v1/libraries/{id}",
    tag = "libraries",
    params(("id" = i32, Path, description = "Library id")),
    request_body = LibraryBody,
    responses(
        (status = 200, description = "The renamed library", body = Library),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such library", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_library(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<LibraryBody>,
) -> Result<Json<Library>, Problem> {
    if !caller.allows_in_library(Permission::SettingsManage, id) {
        return Err(Problem::not_found());
    }
    let name = valid_name(&body.name)?;
    let before = db::libraries::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    let updated = db::libraries::rename(&state.db, id, name)
        .await
        .map_err(library_error)?;
    let event = Event::new("library.updated").target("library", id).data(
        Changes::new()
            .field("name", &before.library.name, &updated.library.name)
            .into_data(),
    );
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(updated.into()))
}

/// Deletes an empty library. Any scoped user grants referencing this library
/// lose access to it without escalating to all libraries.
#[utoipa::path(
    delete,
    path = "/api/v1/libraries/{id}",
    tag = "libraries",
    params(("id" = i32, Path, description = "Library id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such library", body = Problem),
        (status = 409, description = "A root folder belongs to this library", body = Problem),
    )
)]
pub async fn delete_library(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    if !caller.allows_in_library(Permission::SettingsManage, id) {
        return Err(Problem::not_found());
    }
    let current = db::libraries::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    let affected_users = db::libraries::users_scoped_to_library(&state.db, id)
        .await
        .map_err(failed)?;
    db::libraries::delete(&state.db, id)
        .await
        .map_err(library_error)?;
    for user_id in affected_users {
        state.publish_permissions_changed(&user_id);
    }
    let event = Event::new("library.deleted")
        .target("library", id)
        .data(json!({ "name": current.library.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}
