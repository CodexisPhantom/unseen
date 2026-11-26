//! The frontend build, embedded in the binary (ADR 0003).

use axum::http::{HeaderValue, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::error::Problem;

// A debug build reads the folder at run time; a release build embeds it.
// `allow_missing` lets the crate compile before the frontend is built.
#[derive(RustEmbed)]
#[folder = "../../web/build"]
#[allow_missing = true]
struct Assets;

/// Router fallback: an embedded file, or `index.html` so the SPA can route
/// a deep link such as `/library`.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if is_reserved(path) {
        return Problem::not_found().into_response();
    }
    file(path)
        .or_else(|| file("index.html"))
        .unwrap_or_else(|| Problem::not_found().into_response())
}

/// Paths that never fall back to `index.html`: the API prefixes, and
/// SvelteKit's asset folder (a missing asset must be a 404, not HTML).
fn is_reserved(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or_default();
    ["api", "radarr", "sonarr", "_app"]
        .iter()
        .any(|prefix| first.eq_ignore_ascii_case(prefix))
        && Assets::get(path).is_none()
}

fn file(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let content_type = HeaderValue::from_str(file.metadata.mimetype()).ok()?;
    let headers = [
        (header::CONTENT_TYPE, content_type),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control(path)),
        ),
    ];
    Some((headers, file.data).into_response())
}

/// SvelteKit puts content-hashed files in `_app/immutable/`; they never
/// change. Everything else, `index.html` included, is revalidated.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_prefixes_and_missing_assets_are_reserved() {
        for path in [
            "api",
            "api/v1/nope",
            "radarr/api/v3/movie",
            "sonarr",
            "_app/immutable/gone.js",
        ] {
            assert!(is_reserved(path), "{path}");
        }
        for path in ["", "library", "apiary", "settings/radarr"] {
            assert!(!is_reserved(path), "{path}");
        }
    }

    #[test]
    fn only_hashed_assets_are_immutable() {
        assert!(cache_control("_app/immutable/chunks/a1b2.js").contains("immutable"));
        assert_eq!(cache_control("index.html"), "no-cache");
        assert_eq!(cache_control("_app/version.json"), "no-cache");
    }
}
