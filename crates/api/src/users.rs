//! Users: accounts, roles, direct grants (docs/15-permissions.md, "Owner
//! role", "Direct grants" and "Escalation rules").

use std::collections::BTreeSet;

use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use better_auth::integrations::axum::AxumIntegration;
use better_auth::{Argon2PasswordHasher, PasswordHasher};
use chrono::SecondsFormat;
use db::roles::RoleDetail;
use db::users::{UserDetail, UserError};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use unseen_core::permission::{EffectivePermissions, Permission, can_modify_user};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Caller, Require, UsersManage};
use crate::error::Problem;
use crate::me::{MeRole, PermissionName};
use crate::roles::{escalation, known, parse_permissions};

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
    created_at: String,
}

impl From<UserDetail> for User {
    fn from(detail: UserDetail) -> Self {
        let permissions = known(&detail.permissions).map(Permission::as_str).collect();
        Self {
            is_owner: is_owner(&detail),
            id: detail.user.id,
            name: detail.user.name,
            email: detail.user.email,
            disabled: detail.disabled,
            roles: detail
                .roles
                .into_iter()
                .map(|role| MeRole {
                    id: role.id,
                    name: role.name,
                    is_owner: role.is_owner,
                })
                .collect(),
            permissions,
            created_at: detail
                .user
                .created_at
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewUser {
    name: String,
    email: String,
    password: String,
    #[serde(default)]
    role_ids: Vec<i32>,
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
    /// Replaces the roles.
    role_ids: Option<Vec<i32>>,
}

#[derive(Deserialize, ToSchema)]
pub struct DirectGrants {
    /// Replaces the direct grants.
    #[schema(value_type = Vec<PermissionName>)]
    permissions: Vec<String>,
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

/// Roles whose assignment changes must be held in full by the editor, and
/// only an Owner hands out or takes away the Owner role.
fn check_role_changes(
    caller: &Caller,
    all: &[RoleDetail],
    current: &BTreeSet<i32>,
    wanted: &BTreeSet<i32>,
) -> Result<(), Problem> {
    for id in wanted {
        if !all.iter().any(|role| role.role.id == *id) {
            return Err(Problem::validation(
                "roleIds",
                &format!("unknown role {id}"),
            ));
        }
    }
    for role in all
        .iter()
        .filter(|role| current.contains(&role.role.id) != wanted.contains(&role.role.id))
    {
        if role.role.is_owner && !caller.is_owner() {
            return Err(Problem::refused(
                "owner_only",
                "Only an Owner can give or take the Owner role",
            ));
        }
        escalation(caller, known(&role.permissions))?;
    }
    Ok(())
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

fn role_ids(detail: &UserDetail) -> BTreeSet<i32> {
    detail.roles.iter().map(|role| role.id).collect()
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
    headers: HeaderMap,
    Json(body): Json<NewUser>,
) -> Result<(StatusCode, Json<User>), Problem> {
    let name = valid_name(&body.name)?;
    let email = valid_email(&body.email)?;
    valid_password(&body.password)?;
    let wanted: BTreeSet<i32> = body.role_ids.iter().copied().collect();
    let roles = db::roles::list(&state.db).await.map_err(failed)?;
    check_role_changes(&caller, &roles, &BTreeSet::new(), &wanted)?;

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
    db::users::set_roles(&state.db, id, &wanted)
        .await
        .map_err(failed)?;
    tracing::info!(user = id, "user created");
    Ok((StatusCode::CREATED, Json(load(&state, id).await?.into())))
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

    let current = role_ids(&target);
    let wanted = body
        .role_ids
        .as_ref()
        .map(|ids| ids.iter().copied().collect());
    let roles = db::roles::list(&state.db).await.map_err(failed)?;
    if let Some(wanted) = &wanted {
        check_role_changes(&caller, &roles, &current, wanted)?;
    }
    let keeps_owner_role = wanted.as_ref().is_none_or(|wanted: &BTreeSet<i32>| {
        roles
            .iter()
            .any(|role| role.role.is_owner && wanted.contains(&role.role.id))
    });
    let enabled_after = !body.disabled.unwrap_or(target.disabled);
    keep_last_owner(&state, &target, keeps_owner_role && enabled_after).await?;

    if name.is_some() || email.is_some() {
        db::users::update_profile(&state.db, &id, name, email)
            .await
            .map_err(user_error)?;
    }
    if let Some(wanted) = &wanted {
        db::users::set_roles(&state.db, &id, wanted)
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
    Ok(Json(load(&state, &id).await?.into()))
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
    Path(id): Path<String>,
) -> Result<StatusCode, Problem> {
    let target = load(&state, &id).await?;
    may_modify(&caller, &target)?;
    keep_last_owner(&state, &target, false).await?;
    db::users::delete(&state.db, &id).await.map_err(failed)?;
    state.policy.enable(&id);
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
    Path(id): Path<String>,
    Json(body): Json<DirectGrants>,
) -> Result<Json<User>, Problem> {
    let target = load(&state, &id).await?;
    may_modify(&caller, &target)?;
    let wanted = parse_permissions(&body.permissions)?;
    let current: BTreeSet<Permission> = known(&target.permissions).collect();
    escalation(&caller, wanted.symmetric_difference(&current).copied())?;
    let stored = wanted.iter().map(|p| p.as_str().to_owned()).collect();
    db::users::set_permissions(&state.db, &id, &stored)
        .await
        .map_err(failed)?;
    Ok(Json(load(&state, &id).await?.into()))
}
