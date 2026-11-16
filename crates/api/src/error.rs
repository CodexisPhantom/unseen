//! RFC 9457 problem details (docs/05-api.md).

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct Problem {
    /// URI that identifies the problem kind.
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: u16,
    /// Stable machine-readable string. Clients switch on this.
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl Problem {
    pub fn new(status: StatusCode, code: &str, title: &str) -> Self {
        Self {
            kind: format!("https://unseen.invalid/problems/{code}"),
            title: title.into(),
            status: status.as_u16(),
            code: code.into(),
            detail: None,
            errors: Vec::new(),
        }
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found")
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let headers = [(header::CONTENT_TYPE, "application/problem+json")];
        (status, headers, Json(self)).into_response()
    }
}
