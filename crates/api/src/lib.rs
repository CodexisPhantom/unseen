//! HTTP API: routes, the OpenAPI document and the Axum router.

pub mod auth;
pub mod error;
mod health;
mod web;

use axum::Router;
use axum::extract::FromRef;
use better_auth::AuthResult;
use better_auth::integrations::axum::AxumIntegration;
use db::DatabaseConnection;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::Problem;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub auth: auth::Auth,
    pub policy: auth::Policy,
}

impl AppState {
    /// `db` must be migrated: the auth instance reads its tables.
    pub async fn new(
        db: DatabaseConnection,
        secret: &str,
        base_url: Option<&str>,
    ) -> AuthResult<Self> {
        let (auth, policy) = auth::build(db.clone(), secret, base_url).await?;
        Ok(Self { db, auth, policy })
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

/// Every route is registered here once, so the router and the OpenAPI
/// document cannot disagree.
fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).routes(routes!(health::health))
}

/// The OpenAPI document. `just gen` writes it to `openapi.json`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().into_openapi()
}

pub fn router(state: AppState) -> Router {
    let (router, _) = routes().split_for_parts();
    // The auth routes are the library's own; they are not in our OpenAPI
    // document.
    let auth = state.auth.clone().axum_router_with_state();
    router
        .nest("/api/auth", auth)
        .fallback(web::serve)
        .with_state(state)
}
