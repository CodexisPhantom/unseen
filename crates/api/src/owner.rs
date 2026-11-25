//! Ownership transfer (docs/15-permissions.md, "Owner role").

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use better_auth::{Argon2PasswordHasher, PasswordHasher};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::Caller;
use crate::audit::{Actor, ClientIp, Event, record};
use crate::error::Problem;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    /// An enabled user who receives the Owner role.
    target_user_id: String,
    /// The caller's own password.
    password: String,
    /// A role the caller keeps instead of the Owner role. None means no role.
    keep_role_id: Option<i32>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "owner transfer");
    Problem::internal()
}

/// Hands the Owner role to another enabled user. The caller loses it and may
/// keep another role. Requires the Owner role and the caller's password; this
/// is the one documented exception to the one-permission rule.
#[utoipa::path(
    post,
    path = "/api/v1/owner/transfer",
    tag = "owner",
    request_body = Transfer,
    extensions(("x-owner-only" = json!(true))),
    responses(
        (status = 204, description = "Transferred"),
        (status = 403, description = "The caller is not an Owner, or the password is wrong", body = Problem),
        (status = 404, description = "No such user", body = Problem),
        (status = 409, description = "The target user is disabled", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn transfer(
    caller: Caller,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<Transfer>,
) -> Result<StatusCode, Problem> {
    if !caller.is_owner() {
        return Err(Problem::refused(
            "owner_only",
            "Only an Owner can transfer ownership",
        ));
    }
    let hash = db::users::password_hash(&state.db, caller.user_id())
        .await
        .map_err(failed)?;
    let correct = match hash {
        Some(hash) => Argon2PasswordHasher
            .verify(&hash, &body.password)
            .await
            .map_err(failed)?,
        None => false,
    };
    if !correct {
        return Err(Problem::refused("wrong_password", "The password is wrong"));
    }
    if body.target_user_id == caller.user_id() {
        return Err(Problem::validation("targetUserId", "must be another user"));
    }
    let target = db::users::find(&state.db, &body.target_user_id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    if target.disabled {
        return Err(Problem::conflict(
            "target_disabled",
            "The target user is disabled",
        ));
    }
    let kept = match body.keep_role_id {
        Some(id) => {
            let roles = db::roles::list(&state.db).await.map_err(failed)?;
            let role = roles
                .into_iter()
                .find(|role| role.role.id == id)
                .ok_or_else(|| Problem::validation("keepRoleId", "unknown role"))?;
            if role.role.is_owner {
                return Err(Problem::validation(
                    "keepRoleId",
                    "must not be the Owner role",
                ));
            }
            Some(role.role.name)
        }
        None => None,
    };

    db::users::transfer_owner(
        &state.db,
        caller.user_id(),
        &target.user.id,
        body.keep_role_id,
    )
    .await
    .map_err(failed)?;
    tracing::info!(from = caller.user_id(), to = %target.user.id, "ownership transferred");
    let event = Event::new("owner.transferred")
        .target("user", &target.user.id)
        .data(json!({
            "from": caller.user_id(),
            "to": target.user.id,
            "keptRole": kept,
        }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}
