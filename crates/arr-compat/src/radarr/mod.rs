//! The Radarr v3 API as companion tools see it (docs/14-compatibility.md,
//! section D): the resources in [`dto`] and the mapping in [`map`].

pub mod dto;
pub mod map;

/// The Radarr release whose OpenAPI document is pinned in
/// `fixtures/radarr/openapi-<version>.json`. `GET /system/status` reports it.
/// Raise it together with the pinned document.
pub const RADARR_VERSION: &str = "7.0.3.10918";

/// Where the Radarr API lives, as `system/status` reports in `urlBase`.
pub const URL_BASE: &str = "/radarr";
