//! HTTP API: routes, the OpenAPI document and the Axum router.

pub mod error;
mod health;
mod web;

use axum::Router;
use db::DatabaseConnection;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::Problem;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
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
    router.fallback(web::serve).with_state(state)
}
