//! Roles and the permission catalog (docs/15-permissions.md, "Custom roles"
//! and "Escalation rules").

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use db::roles::{RoleChanges, RoleDetail, RoleError};
use serde::{Deserialize, Serialize};
use unseen_core::permission::Permission;
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{Caller, Require, RolesManage, UsersManage};
use crate::error::Problem;
use crate::me::PermissionName;

const NAME_MAX: usize = 64;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    id: i32,
    name: String,
    description: Option<String>,
    /// The built-in Owner role. Read-only; it holds the whole catalog.
    is_owner: bool,
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<&'static str>,
    /// Number of users who hold the role.
    users: u64,
}

impl From<RoleDetail> for Role {
    fn from(detail: RoleDetail) -> Self {
        let permissions = if detail.role.is_owner {
            Permission::ALL.iter().map(|p| p.as_str()).collect()
        } else {
            known(&detail.permissions).map(Permission::as_str).collect()
        };
        Self {
            id: detail.role.id,
            name: detail.role.name,
            description: detail.role.description,
            is_owner: detail.role.is_owner,
            permissions,
            users: detail.users,
        }
    }
}

/// The catalog entries among stored strings, in catalog order.
pub(crate) fn known(stored: &BTreeSet<String>) -> impl Iterator<Item = Permission> + '_ {
    Permission::ALL
        .iter()
        .copied()
        .filter(|permission| stored.contains(permission.as_str()))
}

#[derive(Deserialize, ToSchema)]
pub struct NewRole {
    name: String,
    description: Option<String>,
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct RolePatch {
    name: Option<String>,
    /// An empty text clears the description.
    description: Option<String>,
    /// Replaces the whole set.
    #[schema(value_type = Option<Vec<PermissionName>>)]
    permissions: Option<Vec<String>>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeleteParams {
    /// Required when users hold the role.
    confirm: Option<bool>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "roles");
    Problem::internal()
}

fn role_error(error: RoleError) -> Problem {
    match error {
        RoleError::NotFound => Problem::not_found(),
        RoleError::NameTaken => Problem::conflict("role_name_taken", "A role has this name"),
        RoleError::Db(error) => failed(error),
    }
}

fn owner_role() -> Problem {
    Problem::refused("owner_role_readonly", "The Owner role cannot be changed")
}

pub(crate) fn valid_name(name: &str) -> Result<String, Problem> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Problem::validation("name", "must not be empty"));
    }
    if name.chars().count() > NAME_MAX {
        return Err(Problem::validation("name", "must be at most 64 characters"));
    }
    Ok(name.to_owned())
}

pub(crate) fn parse_permissions(names: &[String]) -> Result<BTreeSet<Permission>, Problem> {
    names
        .iter()
        .map(|name| {
            name.parse().map_err(|_| {
                Problem::validation("permissions", &format!("unknown permission `{name}`"))
            })
        })
        .collect()
}

/// A role may only hold permissions its editor holds.
pub(crate) fn escalation(
    caller: &Caller,
    permissions: impl IntoIterator<Item = Permission>,
) -> Result<(), Problem> {
    match permissions.into_iter().find(|p| !caller.has(*p)) {
        Some(missing) => Err(Problem::forbidden(missing)),
        None => Ok(()),
    }
}

fn strings(permissions: &BTreeSet<Permission>) -> BTreeSet<String> {
    permissions.iter().map(|p| p.as_str().to_owned()).collect()
}

async fn load(state: &AppState, id: i32) -> Result<RoleDetail, Problem> {
    db::roles::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

/// The permission catalog, in catalog order. The list is public knowledge
/// inside the app: the users page needs it for direct grants.
#[utoipa::path(
    get,
    path = "/api/v1/permissions",
    tag = "users",
    responses(
        (status = 200, description = "Every permission", body = Vec<PermissionName>),
        (status = 401, description = "Not signed in", body = Problem),
    )
)]
pub async fn list_permissions(_: Caller) -> Json<Vec<&'static str>> {
    Json(Permission::ALL.iter().map(|p| p.as_str()).collect())
}

/// Every role: the Owner role first, then by name.
#[utoipa::path(
    get,
    path = "/api/v1/roles",
    tag = "users",
    responses(
        (status = 200, description = "The roles", body = Vec<Role>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_roles(
    _: Require<RolesManage>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Role>>, Problem> {
    let roles = db::roles::list(&state.db).await.map_err(failed)?;
    Ok(Json(roles.into_iter().map(Role::from).collect()))
}

/// The roles, for the user form. Same list as `/roles`, for a caller who
/// manages users but not roles.
#[utoipa::path(
    get,
    path = "/api/v1/assignable-roles",
    tag = "users",
    responses(
        (status = 200, description = "The roles", body = Vec<Role>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_assignable_roles(
    _: Require<UsersManage>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Role>>, Problem> {
    let roles = db::roles::list(&state.db).await.map_err(failed)?;
    Ok(Json(roles.into_iter().map(Role::from).collect()))
}

/// Creates a role with permissions the caller holds.
#[utoipa::path(
    post,
    path = "/api/v1/roles",
    tag = "users",
    request_body = NewRole,
    responses(
        (status = 201, description = "The new role", body = Role),
        (status = 403, description = "A permission the caller lacks", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_role(
    caller: Require<RolesManage>,
    State(state): State<AppState>,
    Json(body): Json<NewRole>,
) -> Result<(StatusCode, Json<Role>), Problem> {
    let name = valid_name(&body.name)?;
    let permissions = parse_permissions(&body.permissions)?;
    escalation(&caller, permissions.iter().copied())?;
    let description = body.description.as_deref().map(str::trim);
    let created = db::roles::create(
        &state.db,
        &name,
        description.filter(|text| !text.is_empty()),
        &strings(&permissions),
    )
    .await
    .map_err(role_error)?;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// Changes a custom role. Refused for the Owner role, and for a role that
/// holds a permission the caller lacks.
#[utoipa::path(
    patch,
    path = "/api/v1/roles/{id}",
    tag = "users",
    params(("id" = i32, Path, description = "Role id")),
    request_body = RolePatch,
    responses(
        (status = 200, description = "The changed role", body = Role),
        (status = 403, description = "The Owner role, or a permission the caller lacks", body = Problem),
        (status = 404, description = "No such role", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_role(
    caller: Require<RolesManage>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<RolePatch>,
) -> Result<Json<Role>, Problem> {
    let current = load(&state, id).await?;
    if current.role.is_owner {
        return Err(owner_role());
    }
    escalation(&caller, known(&current.permissions))?;
    let name = body.name.as_deref().map(valid_name).transpose()?;
    let permissions = body
        .permissions
        .as_deref()
        .map(parse_permissions)
        .transpose()?;
    if let Some(permissions) = &permissions {
        escalation(&caller, permissions.iter().copied())?;
    }
    let changes = RoleChanges {
        name,
        description: body.description.map(|text| text.trim().to_owned()),
        permissions: permissions.as_ref().map(strings),
    };
    let updated = db::roles::update(&state.db, id, changes)
        .await
        .map_err(role_error)?;
    Ok(Json(updated.into()))
}

/// Deletes a custom role. A role that users hold needs `confirm=true`; the
/// users lose it. Refused for the Owner role, and for a role that holds a
/// permission the caller lacks.
#[utoipa::path(
    delete,
    path = "/api/v1/roles/{id}",
    tag = "users",
    params(("id" = i32, Path, description = "Role id"), DeleteParams),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "The Owner role, or a permission the caller lacks", body = Problem),
        (status = 404, description = "No such role", body = Problem),
        (status = 409, description = "Users hold the role and `confirm` is not set", body = Problem),
    )
)]
pub async fn delete_role(
    caller: Require<RolesManage>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(params): Query<DeleteParams>,
) -> Result<StatusCode, Problem> {
    let current = load(&state, id).await?;
    if current.role.is_owner {
        return Err(owner_role());
    }
    escalation(&caller, known(&current.permissions))?;
    if current.users > 0 && params.confirm != Some(true) {
        return Err(Problem::conflict(
            "role_in_use",
            "Users hold this role; confirm to delete it",
        ));
    }
    db::roles::delete(&state.db, id).await.map_err(role_error)?;
    Ok(StatusCode::NO_CONTENT)
}
