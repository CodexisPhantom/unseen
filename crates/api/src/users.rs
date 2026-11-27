//! Users: accounts, roles, direct grants (docs/15-permissions.md, "Owner
//! role", "Direct grants" and "Escalation rules").

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use better_auth::integrations::axum::AxumIntegration;
use better_auth::{Argon2PasswordHasher, PasswordHasher};
use chrono::SecondsFormat;
use db::roles::RoleDetail;
use db::users::{
    PermissionGrant as StoredPermissionGrant, RoleAssignment as StoredRoleAssignment, UserDetail,
    UserError,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use unseenarr_core::permission::{
    EffectivePermissions, LibraryScope, Permission, can_grant_scoped, can_modify_user,
};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Caller, Require, UsersManage};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;
use crate::me::{MeRole, PermissionName, ScopedPermission};
use crate::roles::{known, names, parse_permissions};

const PASSWORD_MIN: usize = 10;
const PASSWORD_MAX: usize = 128;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct User {
    id: String,
    name: String,
    email: Option<String>,
    disabled: bool,
    /// The user holds the Owner role.
    is_owner: bool,
    /// Ordered by name.
    roles: Vec<MeRole>,
    /// Direct grants only; the effective set also has the roles' permissions.
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<&'static str>,
    /// Direct grants with their library scopes, in catalog order.
    scoped_permissions: Vec<ScopedPermission>,
    created_at: String,
}

impl From<UserDetail> for User {
    fn from(detail: UserDetail) -> Self {
        let is_owner = is_owner(&detail);
        let permissions: Vec<&'static str> =
            known(&detail.permissions).map(Permission::as_str).collect();
        let scoped_permissions = known(&detail.permissions)
            .map(|permission| ScopedPermission {
                permission: permission.as_str(),
                library_ids: detail
                    .permission_scopes
                    .get(permission.as_str())
                    .cloned()
                    .flatten(),
            })
            .collect();
        let role_scopes = detail.role_scopes;
        Self {
            is_owner,
            id: detail.user.id,
            name: detail.user.name,
            email: detail.user.email,
            disabled: detail.disabled,
            roles: detail
                .roles
                .into_iter()
                .map(|role| {
                    let library_ids = role_scopes.get(&role.id).cloned().flatten();
                    MeRole {
                        id: role.id,
                        name: role.name,
                        is_owner: role.is_owner,
                        library_ids,
                    }
                })
                .collect(),
            permissions,
            scoped_permissions,
            created_at: detail
                .user
                .created_at
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignment {
    role_id: i32,
    /// Absent or `null` means all libraries; a list restricts the role to those
    /// libraries.
    #[serde(default)]
    library_ids: Option<Vec<i32>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedGrant {
    #[schema(value_type = PermissionName)]
    permission: String,
    /// Absent or `null` means all libraries; a list restricts the grant to
    /// those libraries.
    #[serde(default)]
    library_ids: Option<Vec<i32>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewUser {
    name: String,
    email: String,
    password: String,
    #[serde(default)]
    role_ids: Vec<i32>,
    /// Scoped role assignments. When provided, takes precedence over `roleIds`.
    #[serde(default)]
    role_assignments: Option<Vec<RoleAssignment>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserPatch {
    name: Option<String>,
    email: Option<String>,
    /// Signs the user out everywhere.
    password: Option<String>,
    /// Disabling signs the user out everywhere.
    disabled: Option<bool>,
    /// Replaces the roles (all libraries).
    role_ids: Option<Vec<i32>>,
    /// Replaces the roles with explicit library scopes. Takes precedence over
    /// `roleIds` when provided.
    role_assignments: Option<Vec<RoleAssignment>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectGrants {
    /// Replaces the direct grants (all libraries).
    #[serde(default)]
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<String>,
    /// Replaces the direct grants with explicit library scopes. Takes
    /// precedence over `permissions` when provided.
    #[serde(default)]
    scoped_permissions: Option<Vec<ScopedGrant>>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "users");
    Problem::internal()
}

fn user_error(error: UserError) -> Problem {
    match error {
        UserError::NotFound => Problem::not_found(),
        UserError::EmailTaken => Problem::conflict("email_taken", "A user has this email"),
        UserError::Db(error) => failed(error),
    }
}

fn is_owner(detail: &UserDetail) -> bool {
    detail.roles.iter().any(|role| role.is_owner)
}

async fn load(state: &AppState, id: &str) -> Result<UserDetail, Problem> {
    db::users::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

/// Nobody but an Owner changes an Owner.
fn may_modify(caller: &Caller, target: &UserDetail) -> Result<(), Problem> {
    let target = EffectivePermissions::new(is_owner(target), None::<&str>);
    if can_modify_user(caller.permissions(), &target) {
        Ok(())
    } else {
        Err(Problem::refused(
            "owner_only",
            "Only an Owner can change an Owner",
        ))
    }
}

/// The last enabled Owner stays an enabled Owner.
async fn keep_last_owner(
    state: &AppState,
    target: &UserDetail,
    stays_enabled_owner: bool,
) -> Result<(), Problem> {
    if stays_enabled_owner || target.disabled || !is_owner(target) {
        return Ok(());
    }
    let owners = db::users::enabled_owners(&state.db).await.map_err(failed)?;
    if owners.iter().all(|id| *id == target.user.id) {
        return Err(Problem::conflict(
            "last_owner",
            "The last Owner cannot be disabled, deleted or lose the Owner role",
        ));
    }
    Ok(())
}

type RoleScopeMap = BTreeMap<i32, Option<BTreeSet<i32>>>;

fn check_library_ids(
    known_libraries: &BTreeSet<i32>,
    ids: &Option<BTreeSet<i32>>,
    field: &str,
) -> Result<(), Problem> {
    if let Some(ids) = ids {
        for id in ids {
            if !known_libraries.contains(id) {
                return Err(Problem::validation(field, &format!("unknown library {id}")));
            }
        }
    }
    Ok(())
}

fn scoped_escalation(
    caller: &Caller,
    permission: Permission,
    scope: &LibraryScope,
) -> Result<(), Problem> {
    if can_grant_scoped(caller.permissions(), permission, scope) {
        Ok(())
    } else {
        Err(Problem::forbidden(permission))
    }
}

/// Roles whose assignment or library scope changes must be held in full (both
/// permissions and library scopes) by the editor, and only an Owner hands out
/// or takes away the Owner role. The Owner role cannot be scoped.
fn check_role_assignment_changes(
    caller: &Caller,
    all: &[RoleDetail],
    known_libraries: &BTreeSet<i32>,
    current: &BTreeMap<i32, Option<BTreeSet<i32>>>,
    wanted: &BTreeMap<i32, Option<BTreeSet<i32>>>,
    field: &str,
) -> Result<(), Problem> {
    for (id, libs) in wanted {
        let Some(role) = all.iter().find(|role| role.role.id == *id) else {
            return Err(Problem::validation(field, &format!("unknown role {id}")));
        };
        if role.role.is_owner && libs.is_some() {
            return Err(Problem::validation(
                field,
                "the Owner role applies to all libraries and cannot be scoped",
            ));
        }
        check_library_ids(known_libraries, libs, field)?;
    }
    for role in all
        .iter()
        .filter(|role| current.get(&role.role.id) != wanted.get(&role.role.id))
    {
        if role.role.is_owner && !caller.is_owner() {
            return Err(Problem::refused(
                "owner_only",
                "Only an Owner can give or take the Owner role",
            ));
        }
        for permission in known(&role.permissions) {
            if let Some(old_libs) = current.get(&role.role.id) {
                let scope = match old_libs {
                    None => LibraryScope::All,
                    Some(ids) => LibraryScope::Libraries(ids.clone()),
                };
                scoped_escalation(caller, permission, &scope)?;
            }
            if let Some(new_libs) = wanted.get(&role.role.id) {
                let scope = match new_libs {
                    None => LibraryScope::All,
                    Some(ids) => LibraryScope::Libraries(ids.clone()),
                };
                scoped_escalation(caller, permission, &scope)?;
            }
        }
    }
    Ok(())
}

fn current_role_map(detail: &UserDetail) -> BTreeMap<i32, Option<BTreeSet<i32>>> {
    detail
        .roles
        .iter()
        .map(|role| {
            let libs = detail
                .role_scopes
                .get(&role.id)
                .cloned()
                .flatten()
                .map(|ids| ids.into_iter().collect());
            (role.id, libs)
        })
        .collect()
}

fn to_stored_assignments(
    wanted: &BTreeMap<i32, Option<BTreeSet<i32>>>,
) -> Vec<StoredRoleAssignment> {
    wanted
        .iter()
        .map(|(&role_id, library_ids)| StoredRoleAssignment {
            role_id,
            library_ids: library_ids.clone(),
        })
        .collect()
}

fn assignments_from_request(
    role_assignments: Option<&[RoleAssignment]>,
    role_ids: &[i32],
) -> (BTreeMap<i32, Option<BTreeSet<i32>>>, &'static str) {
    match role_assignments {
        Some(entries) => {
            let mut map: BTreeMap<i32, Option<BTreeSet<i32>>> = BTreeMap::new();
            for entry in entries {
                let scope = entry
                    .library_ids
                    .as_ref()
                    .map(|ids| ids.iter().copied().collect::<BTreeSet<i32>>());
                map.entry(entry.role_id)
                    .and_modify(|existing| match (existing, &scope) {
                        (None, _) => {}
                        (slot, None) => *slot = None,
                        (Some(a), Some(b)) => a.extend(b.iter().copied()),
                    })
                    .or_insert(scope);
            }
            (map, "roleAssignments")
        }
        None => (role_ids.iter().map(|&id| (id, None)).collect(), "roleIds"),
    }
}

async fn known_library_ids(state: &AppState) -> Result<BTreeSet<i32>, Problem> {
    Ok(db::libraries::list(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|detail| detail.library.id)
        .collect())
}

fn valid_name(name: &str) -> Result<String, Problem> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Problem::validation("name", "must not be empty"));
    }
    Ok(name.to_owned())
}

fn valid_email(email: &str) -> Result<String, Problem> {
    let email = email.trim().to_lowercase();
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && domain.contains('.') => Ok(email),
        _ => Err(Problem::validation("email", "must be an email address")),
    }
}

fn valid_password(password: &str) -> Result<(), Problem> {
    let length = password.chars().count();
    if !(PASSWORD_MIN..=PASSWORD_MAX).contains(&length) {
        return Err(Problem::validation(
            "password",
            &format!("must have {PASSWORD_MIN} to {PASSWORD_MAX} characters"),
        ));
    }
    Ok(())
}

/// The names of a user's roles, ordered by name.
fn role_names(detail: &UserDetail) -> Vec<&str> {
    detail.roles.iter().map(|role| role.name.as_str()).collect()
}

/// Every user, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/users",
    tag = "users",
    responses(
        (status = 200, description = "The users", body = Vec<User>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_users(
    _: Require<UsersManage>,
    State(state): State<AppState>,
) -> Result<Json<Vec<User>>, Problem> {
    let users = db::users::list(&state.db).await.map_err(failed)?;
    Ok(Json(users.into_iter().map(User::from).collect()))
}

/// Creates a user who can sign in with the given password. Only roles the
/// caller holds in full can be assigned.
#[utoipa::path(
    post,
    path = "/api/v1/users",
    tag = "users",
    request_body = NewUser,
    responses(
        (status = 201, description = "The new user", body = User),
        (status = 403, description = "A role the caller does not hold in full", body = Problem),
        (status = 409, description = "The email is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_user(
    caller: Require<UsersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    Json(body): Json<NewUser>,
) -> Result<(StatusCode, Json<User>), Problem> {
    let name = valid_name(&body.name)?;
    let email = valid_email(&body.email)?;
    valid_password(&body.password)?;
    let (wanted, field) =
        assignments_from_request(body.role_assignments.as_deref(), &body.role_ids);
    let roles = db::roles::list(&state.db).await.map_err(failed)?;
    let known_libs = known_library_ids(&state).await?;
    check_role_assignment_changes(
        &caller,
        &roles,
        &known_libs,
        &BTreeMap::new(),
        &wanted,
        field,
    )?;

    let _guard = state.policy.signup_lock().lock().await;
    let existing = db::users::list(&state.db).await.map_err(failed)?;
    if existing
        .iter()
        .any(|user| user.user.email.as_deref() == Some(email.as_str()))
    {
        return Err(user_error(UserError::EmailTaken));
    }
    // The library's own sign-up, so that hashing and validation are the
    // library's. It also opens a session, which is dropped below.
    let mut request = Request::post("/api/auth/sign-up/email")
        .body(Body::from(
            json!({ "name": name, "email": email, "password": body.password }).to_string(),
        ))
        .map_err(failed)?;
    *request.headers_mut() = headers;
    let request_headers = request.headers_mut();
    request_headers.remove(header::COOKIE);
    request_headers.remove(header::CONTENT_LENGTH);
    request_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    state.policy.admit_next_signup();
    let response = state
        .auth
        .clone()
        .axum_router_with_state::<AppState>()
        .with_state(state.clone())
        .oneshot(request)
        .await;
    state.policy.deny_signup();
    let response = response.map_err(failed)?;
    let (parts, response_body) = response.into_parts();
    let bytes = to_bytes(response_body, 1 << 20).await.map_err(failed)?;
    let reply: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !parts.status.is_success() {
        if parts.status.is_server_error() {
            return Err(failed(format!("sign-up failed: {reply}")));
        }
        let mut problem = Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation",
            "Validation failed",
        );
        problem.detail = reply["message"].as_str().map(str::to_owned);
        return Err(problem);
    }
    let id = reply["user"]["id"]
        .as_str()
        .ok_or_else(|| failed("sign-up returned no user id"))?;

    db::users::delete_sessions(&state.db, id)
        .await
        .map_err(failed)?;
    db::users::set_role_assignments(&state.db, id, &to_stored_assignments(&wanted))
        .await
        .map_err(failed)?;
    tracing::info!(user = id, "user created");
    let created = load(&state, id).await?;
    let event = Event::new("user.created").target("user", id).data(json!({
        "name": created.user.name,
        "email": created.user.email,
        "roles": role_names(&created),
    }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// Changes a user. Refused for an Owner unless the caller is an Owner. The
/// last enabled Owner cannot be disabled or lose the Owner role.
#[utoipa::path(
    patch,
    path = "/api/v1/users/{id}",
    tag = "users",
    params(("id" = String, Path, description = "User id")),
    request_body = UserPatch,
    responses(
        (status = 200, description = "The changed user", body = User),
        (status = 403, description = "An Owner, or a role the caller does not hold in full", body = Problem),
        (status = 404, description = "No such user", body = Problem),
        (status = 409, description = "The email is taken, or this is the last Owner", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_user(
    caller: Require<UsersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<String>,
    Json(body): Json<UserPatch>,
) -> Result<Json<User>, Problem> {
    let target = load(&state, &id).await?;
    may_modify(&caller, &target)?;
    let name = body.name.as_deref().map(valid_name).transpose()?;
    let email = body.email.as_deref().map(valid_email).transpose()?;
    if let Some(password) = &body.password {
        valid_password(password)?;
    }

    let current = current_role_map(&target);
    let wanted_with_field: Option<(RoleScopeMap, &'static str)> =
        if let Some(entries) = body.role_assignments.as_deref() {
            Some(assignments_from_request(Some(entries), &[]))
        } else {
            body.role_ids
                .as_deref()
                .map(|ids| assignments_from_request(None, ids))
        };
    let roles = db::roles::list(&state.db).await.map_err(failed)?;
    if let Some((wanted, field)) = &wanted_with_field {
        let known_libs = known_library_ids(&state).await?;
        check_role_assignment_changes(&caller, &roles, &known_libs, &current, wanted, field)?;
    }
    let keeps_owner_role = wanted_with_field.as_ref().is_none_or(|(wanted, _)| {
        roles
            .iter()
            .any(|role| role.role.is_owner && wanted.contains_key(&role.role.id))
    });
    let enabled_after = !body.disabled.unwrap_or(target.disabled);
    keep_last_owner(&state, &target, keeps_owner_role && enabled_after).await?;

    if name.is_some() || email.is_some() {
        db::users::update_profile(&state.db, &id, name, email)
            .await
            .map_err(user_error)?;
    }
    if let Some((wanted, _)) = &wanted_with_field {
        db::users::set_role_assignments(&state.db, &id, &to_stored_assignments(wanted))
            .await
            .map_err(failed)?;
    }
    if let Some(password) = &body.password {
        let hash = Argon2PasswordHasher.hash(password).await.map_err(failed)?;
        if !db::users::set_password(&state.db, &id, &hash)
            .await
            .map_err(failed)?
        {
            return Err(Problem::validation("password", "this user has no password"));
        }
    }
    if let Some(disabled) = body.disabled {
        db::users::set_disabled(&state.db, &id, disabled)
            .await
            .map_err(failed)?;
        if disabled {
            state.policy.disable(&id);
        } else {
            state.policy.enable(&id);
        }
    }
    // A new password or a disabled account ends the sessions at once.
    if body.password.is_some() || body.disabled == Some(true) {
        db::users::delete_sessions(&state.db, &id)
            .await
            .map_err(failed)?;
    }
    let updated = load(&state, &id).await?;
    if current != current_role_map(&updated) {
        state.publish_permissions_changed(&id);
    }
    let mut changed = Changes::new()
        .field("name", &target.user.name, &updated.user.name)
        .field("email", &target.user.email, &updated.user.email)
        .field("disabled", target.disabled, updated.disabled)
        .field("roles", role_names(&target), role_names(&updated));
    if body.password.is_some() {
        changed = changed.secret("password");
    }
    let event = Event::new("user.updated")
        .target("user", &id)
        .data(changed.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(updated.into()))
}

/// Deletes a user and their sessions. Refused for an Owner unless the caller
/// is an Owner, and for the last enabled Owner.
#[utoipa::path(
    delete,
    path = "/api/v1/users/{id}",
    tag = "users",
    params(("id" = String, Path, description = "User id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "An Owner and the caller is not one", body = Problem),
        (status = 404, description = "No such user", body = Problem),
        (status = 409, description = "The last Owner", body = Problem),
    )
)]
pub async fn delete_user(
    caller: Require<UsersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<String>,
) -> Result<StatusCode, Problem> {
    let target = load(&state, &id).await?;
    may_modify(&caller, &target)?;
    keep_last_owner(&state, &target, false).await?;
    // The actor's name is read first: the caller may be the deleted user.
    let actor = Actor::of(&state, &caller).await;
    db::users::delete(&state.db, &id).await.map_err(failed)?;
    state.policy.enable(&id);
    let event = Event::new("user.deleted").target("user", &id).data(json!({
        "name": target.user.name,
        "email": target.user.email,
        "roles": role_names(&target),
    }));
    record(&state, &actor, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces the direct grants of a user. Only permissions the caller holds
/// can be added or taken away.
#[utoipa::path(
    put,
    path = "/api/v1/users/{id}/permissions",
    tag = "users",
    params(("id" = String, Path, description = "User id")),
    request_body = DirectGrants,
    responses(
        (status = 200, description = "The user with the new grants", body = User),
        (status = 403, description = "An Owner, or a permission the caller lacks", body = Problem),
        (status = 404, description = "No such user", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn set_user_permissions(
    caller: Require<UsersManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<String>,
    Json(body): Json<DirectGrants>,
) -> Result<Json<User>, Problem> {
    let target = load(&state, &id).await?;
    may_modify(&caller, &target)?;
    let known_libs = known_library_ids(&state).await?;
    let wanted: BTreeMap<Permission, Option<BTreeSet<i32>>> =
        if let Some(entries) = body.scoped_permissions {
            let mut map: BTreeMap<Permission, Option<BTreeSet<i32>>> = BTreeMap::new();
            for entry in entries {
                let perm: Permission = entry.permission.parse().map_err(|_| {
                    Problem::validation(
                        "scopedPermissions",
                        &format!("unknown permission `{}`", entry.permission),
                    )
                })?;
                let libs = entry
                    .library_ids
                    .map(|ids| ids.into_iter().collect::<BTreeSet<i32>>());
                check_library_ids(&known_libs, &libs, "scopedPermissions")?;
                map.entry(perm)
                    .and_modify(|existing| match (existing, &libs) {
                        (None, _) => {}
                        (slot, None) => *slot = None,
                        (Some(a), Some(b)) => a.extend(b.iter().copied()),
                    })
                    .or_insert(libs);
            }
            map
        } else {
            parse_permissions(&body.permissions)?
                .into_iter()
                .map(|perm| (perm, None))
                .collect()
        };

    let current: BTreeMap<Permission, Option<BTreeSet<i32>>> = known(&target.permissions)
        .map(|perm| {
            let libs = target
                .permission_scopes
                .get(perm.as_str())
                .cloned()
                .flatten()
                .map(|ids| ids.into_iter().collect());
            (perm, libs)
        })
        .collect();

    for perm in current
        .keys()
        .chain(wanted.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        if current.get(&perm) == wanted.get(&perm) {
            continue;
        }
        if let Some(old_libs) = current.get(&perm) {
            let scope = match old_libs {
                None => LibraryScope::All,
                Some(ids) => LibraryScope::Libraries(ids.clone()),
            };
            scoped_escalation(&caller, perm, &scope)?;
        }
        if let Some(new_libs) = wanted.get(&perm) {
            let scope = match new_libs {
                None => LibraryScope::All,
                Some(ids) => LibraryScope::Libraries(ids.clone()),
            };
            scoped_escalation(&caller, perm, &scope)?;
        }
    }

    let stored: Vec<StoredPermissionGrant> = wanted
        .iter()
        .map(|(perm, library_ids)| StoredPermissionGrant {
            permission: perm.as_str().to_owned(),
            library_ids: library_ids.clone(),
        })
        .collect();
    db::users::set_scoped_permissions(&state.db, &id, &stored)
        .await
        .map_err(failed)?;
    let updated = load(&state, &id).await?;
    if current != wanted {
        state.publish_permissions_changed(&id);
    }
    let event = Event::new("user.permissions_changed")
        .target("user", &id)
        .data(
            Changes::new()
                .field(
                    "permissions",
                    names(&target.permissions),
                    names(&updated.permissions),
                )
                .into_data(),
        );
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(updated.into()))
}
