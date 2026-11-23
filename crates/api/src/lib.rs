//! HTTP API: routes, the OpenAPI document and the Axum router.

pub mod access;
pub mod auth;
pub mod error;
mod health;
mod me;
pub mod setup;
mod web;

use axum::Router;
use axum::extract::FromRef;
use better_auth::AuthResult;
use better_auth::integrations::axum::AxumIntegration;
use db::DatabaseConnection;
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
}

impl AppState {
    /// `db` must be migrated: the auth instance reads its tables.
    pub async fn new(
        db: DatabaseConnection,
        secret: &str,
        base_url: Option<&str>,
    ) -> AuthResult<Self> {
        let (auth, policy) = auth::build(db.clone(), secret, base_url).await?;
        Ok(Self {
            db,
            auth,
            policy,
            setup: setup::Setup::default(),
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
}

/// The OpenAPI document. `just gen` writes it to `openapi.json`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().into_openapi()
}

pub fn router(state: AppState) -> Router {
    let api = routes().into_router(&state);
    // The auth routes are the library's own; they are not in our OpenAPI
    // document.
    let auth = state.auth.clone().axum_router_with_state();
    api.nest("/api/auth", auth)
        .fallback(web::serve)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            setup::gate,
        ))
        .with_state(state)
}
