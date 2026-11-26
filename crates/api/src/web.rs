//! The frontend build, embedded in the binary (ADR 0003).

use axum::extract::State;
use axum::http::{HeaderValue, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::AppState;
use crate::error::Problem;

// A debug build reads the folder at run time; a release build embeds it.
// `allow_missing` lets the crate compile before the frontend is built.
#[derive(RustEmbed)]
#[folder = "../../web/build"]
#[allow_missing = true]
struct Assets;

/// Router fallback: an embedded file, or `index.html` so the SPA can route
/// a deep link such as `/library`.
pub async fn serve(State(state): State<AppState>, uri: Uri) -> Response {
    let base_path = state.base_path.clone();
    let path = uri.path().trim_start_matches('/');
    if is_reserved(path) {
        return Problem::not_found().into_response();
    }
    if let Some(response) = file(path) {
        return response;
    }
    let Some(index) = Assets::get("index.html") else {
        return Problem::not_found().into_response();
    };
    let html = String::from_utf8_lossy(&index.data);
    let html = inject(&html, &base_path);
    html_response(&html)
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

fn inject(html: &str, base_path: &str) -> String {
    let meta = format!(r#"<meta name="unseen-base" content="{base_path}" />"#);
    let mut html = html.replacen("<head>", &format!("<head>{meta}"), 1);
    if !html.contains(&meta) {
        // No <head>: still expose the base path.
        html = format!("{meta}{html}");
    }
    if base_path.is_empty() {
        return html;
    }
    let mut out = html.replace("href=\"/", &format!("href=\"{base_path}/"));
    out = out.replace("src=\"/", &format!("src=\"{base_path}/"));
    out = out.replace("import(\"/", &format!("import(\"{base_path}/"));
    out = out.replace("base: \"\"", &format!("base: \"{base_path}\""));
    out
}

fn html_response(html: &str) -> Response {
    let headers = [
        (header::CONTENT_TYPE, HeaderValue::from_static("text/html")),
        (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
    ];
    (headers, html.to_owned()).into_response()
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

    const SAMPLE: &str = r#"<html><head><title>t</title></head><body><link href="/favicon.svg"><script src="/x.js"></script><script>import("/_app/start.js");base: ""</script></body></html>"#;

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

    #[test]
    fn inject_prefixes_asset_urls_and_base() {
        let out = inject(SAMPLE, "/unseen");
        assert!(out.contains(r#"<meta name="unseen-base" content="/unseen" />"#));
        assert!(out.contains(r#"href="/unseen/favicon.svg""#));
        assert!(out.contains(r#"src="/unseen/x.js""#));
        assert!(out.contains(r#"import("/unseen/_app/start.js")"#));
        assert!(out.contains(r#"base: "/unseen""#));
    }

    #[test]
    fn inject_at_root_only_adds_meta() {
        let out = inject(SAMPLE, "");
        assert!(out.contains(r#"<meta name="unseen-base" content="" />"#));
        assert!(out.contains(r#"href="/favicon.svg""#));
        assert!(out.contains(r#"base: """#));
        assert!(!out.contains("//"));
    }
}
