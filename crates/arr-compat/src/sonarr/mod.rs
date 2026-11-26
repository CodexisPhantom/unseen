//! The Sonarr v3 API as companion tools see it (docs/14-compatibility.md,
//! section D "Sonarr endpoints (M8)"): the resources in [`dto`] and the
//! mapping in [`map`].

pub mod dto;
pub mod map;

/// The Sonarr release whose OpenAPI document is pinned in
/// `fixtures/sonarr/openapi-<version>.json`. `GET /system/status` reports it.
/// Raise it together with the pinned document.
pub const SONARR_VERSION: &str = "5.0.0.3041";

/// Where the Sonarr API lives, as `system/status` reports in `urlBase`.
pub const URL_BASE: &str = "/sonarr";
