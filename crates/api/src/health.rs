use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// Version of the running binary.
    version: String,
    database: DatabaseStatus,
    drift_probe: bool,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DatabaseStatus {
    Ok,
    Error,
}

/// Health check. Public.
#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "system",
    responses(
        (status = 200, description = "The service works", body = Health),
        (status = 503, description = "The database does not answer", body = Health),
    )
)]
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let (status, database) = match state.db.ping().await {
        Ok(()) => (StatusCode::OK, DatabaseStatus::Ok),
        Err(error) => {
            tracing::error!(%error, "database ping failed");
            (StatusCode::SERVICE_UNAVAILABLE, DatabaseStatus::Error)
        }
    };
    let version = env!("CARGO_PKG_VERSION").into();
    (
        status,
        Json(Health {
            version,
            database,
            drift_probe: true,
        }),
    )
}
