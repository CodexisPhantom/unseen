//! HTTP API: routes, the OpenAPI document and the Axum router.

pub mod access;
mod api_keys;
pub mod audit;
pub mod auth;
pub mod error;
mod health;
mod me;
mod roles;
pub mod setup;
mod users;
mod web;

use axum::Router;
use axum::extract::FromRef;
use better_auth::integrations::axum::AxumIntegration;
use better_auth::{AuthError, AuthResult};
use db::DatabaseConnection;
use unseen_core::permission::Permission;
use utoipa::OpenApi;
use utoipa_axum::routes;

use crate::access::ApiRouter;
use crate::error::Problem;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub auth: auth::Auth,
    pub policy: auth::Policy,
    pub setup: setup::Setup,
    /// Encrypts the integration secrets (ADR 0011).
    pub secrets: db::secret::SecretKey,
}

impl AppState {
    /// `db` must be migrated: the auth instance reads its tables.
    pub async fn new(
        db: DatabaseConnection,
        secret: &str,
        base_url: Option<&str>,
    ) -> AuthResult<Self> {
        let (auth, policy) = auth::build(db.clone(), secret, base_url).await?;
        let disabled = db::users::disabled_ids(&db)
            .await
            .map_err(|error| AuthError::internal(format!("load disabled users: {error}")))?;
        for id in &disabled {
            policy.disable(id);
        }
        Ok(Self {
            db,
            auth,
            policy,
            setup: setup::Setup::default(),
            secrets: db::secret::SecretKey::derive(secret),
        })
    }
}

/// The library's extractors and router find the auth instance through this.
impl FromRef<AppState> for auth::Auth {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

#[derive(OpenApi)]
#[openapi(info(title = "Unseen API"), components(schemas(Problem)))]
struct ApiDoc;

/// Every API route is registered here once with its access rule, so the
/// router, runtime enforcement and `x-permission` in OpenAPI cannot disagree.
pub fn routes() -> ApiRouter {
    ApiRouter::with_openapi(ApiDoc::openapi())
        .public(routes!(health::health))
        .public(routes!(setup::status, setup::create))
        .authenticated(routes!(me::me))
        .authenticated(routes!(roles::list_permissions))
        .authenticated(routes!(api_keys::list_own, api_keys::create_own))
        .authenticated(routes!(api_keys::revoke_own))
        .require(Permission::ApikeysManage, routes!(api_keys::list_all))
        .require(Permission::ApikeysManage, routes!(api_keys::revoke_any))
        .require(
            Permission::UsersManage,
            routes!(roles::list_assignable_roles),
        )
        .require(
            Permission::RolesManage,
            routes!(roles::list_roles, roles::create_role),
        )
        .require(
            Permission::RolesManage,
            routes!(roles::update_role, roles::delete_role),
        )
        .require(
            Permission::UsersManage,
            routes!(users::list_users, users::create_user),
        )
        .require(
            Permission::UsersManage,
            routes!(users::update_user, users::delete_user),
        )
        .require(
            Permission::UsersManage,
            routes!(users::set_user_permissions),
        )
        .require(Permission::SystemAudit, routes!(audit::list))
}

/// The OpenAPI document. `just gen` writes it to `openapi.json`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().into_openapi()
}

pub fn router(state: AppState) -> Router {
    let api = routes().into_router(&state);
    // The auth routes are the library's own; they are not in our OpenAPI
    // document.
    let auth =
        state
            .auth
            .clone()
            .axum_router_with_state()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                audit::auth_events,
            ));
    api.nest("/api/auth", auth)
        .fallback(web::serve)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            setup::gate,
        ))
        .with_state(state)
}
