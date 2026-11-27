//! The current user: who they are and what they may do (docs/15-permissions.md).

use std::borrow::Cow;

use axum::Json;
use axum::extract::State;
use better_auth::seaorm::sea_orm::EntityTrait;
use db::auth_schema::user;
use serde::Serialize;
use unseenarr_core::permission::Permission;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{ObjectBuilder, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::AppState;
use crate::access::Caller;
use crate::error::Problem;

/// The permission catalog as a string enum in OpenAPI, so the generated
/// TypeScript `Permission` type cannot drift from `core`.
pub struct PermissionName;

impl PartialSchema for PermissionName {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .description(Some(
                "One entry of the permission catalog, `resource:action`.",
            ))
            .enum_values(Some(
                Permission::ALL.iter().map(|permission| permission.as_str()),
            ))
            .into()
    }
}

impl ToSchema for PermissionName {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("Permission")
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedPermission {
    #[schema(value_type = PermissionName)]
    pub(crate) permission: &'static str,
    /// `null` means all libraries; a list restricts the permission to those
    /// libraries.
    pub(crate) library_ids: Option<Vec<i32>>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    user: MeUser,
    /// Ordered by name.
    roles: Vec<MeRole>,
    /// The user holds the Owner role.
    is_owner: bool,
    /// Effective permissions in catalog order: every role plus direct grants.
    /// An Owner has the whole catalog.
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<&'static str>,
    /// Effective permissions with their library scopes, in catalog order.
    scoped_permissions: Vec<ScopedPermission>,
}

#[derive(Serialize, ToSchema)]
pub struct MeUser {
    id: String,
    name: String,
    email: Option<String>,
    #[serde(rename = "twoFactorEnabled")]
    pub two_factor_enabled: bool,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MeRole {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) is_owner: bool,
    /// `null` means all libraries; a list restricts the role assignment to
    /// those libraries.
    pub(crate) library_ids: Option<Vec<i32>>,
}

/// The signed-in user, their roles and effective permissions. Authenticated,
/// no permission.
#[utoipa::path(
    get,
    path = "/api/v1/me",
    tag = "users",
    responses(
        (status = 200, description = "The current user", body = Me),
        (status = 401, description = "Not signed in", body = Problem),
    )
)]
pub async fn me(caller: Caller, State(state): State<AppState>) -> Result<Json<Me>, Problem> {
    let failed = |error: better_auth::seaorm::sea_orm::DbErr| {
        tracing::error!(%error, "load current user");
        Problem::internal()
    };
    let found = user::Entity::find_by_id(caller.user_id())
        .one(&state.db)
        .await
        .map_err(failed)?
        // The session outlived its user, which deletion prevents.
        .ok_or_else(Problem::unauthorized)?;
    let roles = db::access::assigned_roles_of_user(&state.db, caller.user_id())
        .await
        .map_err(failed)?;
    let scoped_permissions = caller
        .permissions()
        .scoped_iter()
        .map(|(permission, scope)| ScopedPermission {
            permission: permission.as_str(),
            library_ids: scope.library_ids().map(|ids| ids.iter().copied().collect()),
        })
        .collect();
    Ok(Json(Me {
        user: MeUser {
            id: found.id,
            name: found.name,
            email: found.email,
            two_factor_enabled: found.two_factor_enabled.unwrap_or(false),
        },
        roles: roles
            .into_iter()
            .map(|assigned| MeRole {
                id: assigned.role.id,
                name: assigned.role.name,
                is_owner: assigned.role.is_owner,
                library_ids: assigned.library_ids,
            })
            .collect(),
        is_owner: caller.is_owner(),
        permissions: caller
            .permissions()
            .iter()
            .map(Permission::as_str)
            .collect(),
        scoped_permissions,
    }))
}
