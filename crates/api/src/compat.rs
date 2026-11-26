//! Shared machinery for *arr compatibility routers (`/radarr/api/v3`, and later
//! `/sonarr/api/v3`): API-key-only auth, case-insensitive path matching,
//! per-route permission enforcement, Radarr-style error bodies and a logged 501
//! fallback (docs/14-compatibility.md, section D).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::marker::PhantomData;
use std::ops::Deref;
use std::task::{Context, Poll};

use axum::Json;
use axum::Router;
use axum::extract::{Extension, FromRequestParts, OriginalUri, Request, State};
use axum::handler::Handler;
use axum::http::request::Parts;
use axum::http::{Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use serde::Serialize;
use serde_json::{Value, json};
use tower::Service;
use unseen_core::permission::Permission;

use crate::AppState;
use crate::access::{
    ApiKeyRejection, Caller, RequiredPermission, caller_from_api_key, presented_api_key,
};

/// Severity of a Radarr/FluentValidation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationSeverity {
    #[default]
    Error,
    Warning,
    Info,
}

/// One entry of Radarr's `400 Bad Request` validation error array, matching the
/// `ValidationFailure` schema in `fixtures/radarr/openapi-7.0.3.10918.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationFailure {
    pub property_name: String,
    pub error_message: String,
    pub attempted_value: Value,
    pub severity: ValidationSeverity,
    pub error_code: Option<String>,
}

impl ValidationFailure {
    #[must_use]
    pub fn new(property_name: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self {
            property_name: property_name.into(),
            error_message: error_message.into(),
            attempted_value: Value::Null,
            severity: ValidationSeverity::Error,
            error_code: None,
        }
    }

    #[must_use]
    pub fn attempted_value(mut self, value: impl Into<Value>) -> Self {
        self.attempted_value = value.into();
        self
    }

    #[must_use]
    pub const fn severity(mut self, severity: ValidationSeverity) -> Self {
        self.severity = severity;
        self
    }

    #[must_use]
    pub fn error_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }
}

/// Radarr-style JSON error responses returned by compatibility routes.
#[derive(Debug, Clone, PartialEq)]
pub enum CompatError {
    Unauthorized,
    Forbidden(Permission),
    NotFound(String),
    BadRequest(String),
    Validation(Vec<ValidationFailure>),
    NotImplemented { method: Method, path: String },
    Internal,
}

impl CompatError {
    /// HTTP 401 with Radarr's `{ "error": "Unauthorized" }` body.
    #[must_use]
    pub const fn unauthorized() -> Self {
        Self::Unauthorized
    }

    /// HTTP 403 naming the missing permission.
    #[must_use]
    pub const fn forbidden(permission: Permission) -> Self {
        Self::Forbidden(permission)
    }

    /// HTTP 404 with Radarr's `{ "message": "NotFound" }` body.
    #[must_use]
    pub fn not_found() -> Self {
        Self::NotFound("NotFound".to_owned())
    }

    /// HTTP 404 with a custom `{ "message": ... }` body.
    #[must_use]
    pub fn not_found_message(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// HTTP 400 with `{ "message": ... }` for malformed non-validation requests.
    #[must_use]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    /// HTTP 400 with a single-element Radarr validation error array.
    #[must_use]
    pub fn validation(property_name: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self::Validation(vec![ValidationFailure::new(property_name, error_message)])
    }

    /// HTTP 400 with a Radarr validation error array.
    #[must_use]
    pub fn validation_errors(errors: Vec<ValidationFailure>) -> Self {
        Self::Validation(errors)
    }

    /// HTTP 501 naming the unimplemented method and path.
    #[must_use]
    pub fn not_implemented(method: &Method, path: impl Into<String>) -> Self {
        Self::NotImplemented {
            method: method.clone(),
            path: path.into(),
        }
    }

    /// HTTP 500 with `{ "message": "Internal server error" }`.
    #[must_use]
    pub const fn internal() -> Self {
        Self::Internal
    }
}

impl IntoResponse for CompatError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Unauthorized" })),
            )
                .into_response(),
            Self::Forbidden(permission) => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "message": "Forbidden",
                    "description": format!("missing permission `{permission}`"),
                    "requiredPermission": permission.as_str(),
                })),
            )
                .into_response(),
            Self::NotFound(message) => {
                (StatusCode::NOT_FOUND, Json(json!({ "message": message }))).into_response()
            }
            Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "message": message }))).into_response()
            }
            Self::Validation(errors) => (StatusCode::BAD_REQUEST, Json(errors)).into_response(),
            Self::NotImplemented { method, path } => (
                StatusCode::NOT_IMPLEMENTED,
                Json(json!({
                    "message": "Not implemented",
                    "method": method.as_str(),
                    "path": path,
                })),
            )
                .into_response(),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": "Internal server error" })),
            )
                .into_response(),
        }
    }
}

/// Authenticated caller resolved strictly from an API key (`X-Api-Key` header
/// or `?apikey=` query parameter). Session cookies are never accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyCaller(Caller);

impl ApiKeyCaller {
    #[must_use]
    pub fn into_caller(self) -> Caller {
        self.0
    }
}

impl Deref for ApiKeyCaller {
    type Target = Caller;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRequestParts<AppState> for ApiKeyCaller {
    type Rejection = CompatError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let caller = authenticate_api_key(parts, state).await?;
        Ok(Self(caller))
    }
}

/// Extractor that requires API-key authentication and permission `P`, returning
/// Radarr-style JSON errors on failure.
#[derive(Debug, Clone)]
pub struct RequireApiKey<P: RequiredPermission>(Caller, PhantomData<P>);

impl<P: RequiredPermission> RequireApiKey<P> {
    #[must_use]
    pub fn into_caller(self) -> Caller {
        self.0
    }
}

impl<P: RequiredPermission> Deref for RequireApiKey<P> {
    type Target = Caller;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<P: RequiredPermission> FromRequestParts<AppState> for RequireApiKey<P> {
    type Rejection = CompatError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let caller = authenticate_api_key(parts, state).await?;
        if !caller.has(P::PERMISSION) {
            return Err(CompatError::forbidden(P::PERMISSION));
        }
        Ok(Self(caller, PhantomData))
    }
}

/// Authenticates the request using only `X-Api-Key` or `?apikey=`, caching the
/// resulting [`Caller`] in request extensions so downstream extractors (such as
/// [`crate::access::Require`]) reuse the single DB load without ever touching
/// session cookies.
async fn authenticate_api_key(parts: &mut Parts, state: &AppState) -> Result<Caller, CompatError> {
    if let Some(caller) = parts.extensions.get::<Caller>()
        && caller.api_key().is_some()
    {
        return Ok(caller.clone());
    }
    let Some(key) = presented_api_key(parts) else {
        return Err(CompatError::unauthorized());
    };
    let caller = caller_from_api_key(state, &key)
        .await
        .map_err(|rejection| match rejection {
            ApiKeyRejection::Unauthorized => CompatError::unauthorized(),
            ApiKeyRejection::Internal => CompatError::internal(),
        })?;
    parts.extensions.insert(caller.clone());
    Ok(caller)
}

async fn require_api_key(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    if let Err(error) = authenticate_api_key(&mut parts, &state).await {
        return error.into_response();
    }
    next.run(Request::from_parts(parts, body)).await
}

async fn enforce_permission(
    State((state, permission)): State<(AppState, Permission)>,
    req: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    let caller = match authenticate_api_key(&mut parts, &state).await {
        Ok(caller) => caller,
        Err(error) => return error.into_response(),
    };
    if !caller.has(permission) {
        return CompatError::forbidden(permission).into_response();
    }
    next.run(Request::from_parts(parts, body)).await
}

/// Request extension holding the original path before case-insensitive
/// normalization so the 501 fallback logs and reports the caller's path.
#[derive(Debug, Clone)]
pub struct OriginalCompatPath(pub String);

async fn unimplemented_fallback(
    method: Method,
    uri: Uri,
    original: Option<Extension<OriginalCompatPath>>,
) -> Response {
    let path = original
        .map(|Extension(OriginalCompatPath(path))| path)
        .unwrap_or_else(|| uri.path().to_owned());
    tracing::warn!(method = %method, path = %path, "unimplemented compat endpoint");
    CompatError::not_implemented(&method, path).into_response()
}

/// One registered compatibility route and its required permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatRouteEntry {
    pub method: Method,
    pub path: String,
    pub permission: Option<Permission>,
}

struct RegisteredCompatRoute {
    path: String,
    permission: Option<Permission>,
    method_router: MethodRouter<AppState>,
}

/// Builder for a compatibility API mounted under a base prefix (such as
/// `/radarr/api/v3`). Every route declares a single [`Permission`], paths match
/// case-insensitively, auth accepts only API keys, and unimplemented paths or
/// methods return 501 JSON after logging a `warn` event.
pub struct CompatRouter {
    base: String,
    routes: Vec<RegisteredCompatRoute>,
    entries: Vec<CompatRouteEntry>,
}

impl Default for CompatRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl CompatRouter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: String::new(),
            routes: Vec::new(),
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_base(base: impl Into<String>) -> Self {
        let base = base.into();
        let normalized = normalize_base(&base);
        Self {
            base: normalized,
            routes: Vec::new(),
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Registers one route requiring `permission`. Static path segments are
    /// normalized to lowercase so case-insensitive requests match cleanly.
    #[must_use]
    pub fn require<H, T>(
        mut self,
        permission: Permission,
        method: Method,
        path: &str,
        handler: H,
    ) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        let normalized = normalize_route_path(path);
        let entry_path = join_base(&self.base, &normalized);
        self.entries.push(CompatRouteEntry {
            method: method.clone(),
            path: entry_path,
            permission: Some(permission),
        });
        self.routes.push(RegisteredCompatRoute {
            path: normalized,
            permission: Some(permission),
            method_router: method_router_for(&method, handler),
        });
        self
    }

    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.routes.extend(other.routes);
        self.entries.extend(other.entries);
        self
    }

    #[must_use]
    pub fn entries(&self) -> &[CompatRouteEntry] {
        &self.entries
    }

    /// Checks that every registered compat route declares a permission and that
    /// no two routes collide (including case-insensitively or across parameter
    /// names).
    pub fn verify(&self) -> Result<(), Vec<String>> {
        verify_entries(&self.entries)
    }

    /// Builds a [`CompatService`] ready to be mounted with
    /// [`Router::nest_service`] (or called directly in tests).
    #[must_use]
    pub fn into_service(self, state: &AppState) -> CompatService {
        let mut router: Router<AppState> = Router::new();
        for route in self.routes {
            // Use `route_layer` so merging multiple HTTP methods on the same
            // path preserves per-method permission checks and routes unknown
            // methods to `method_not_allowed_fallback`.
            let method_router = match route.permission {
                Some(permission) => {
                    route
                        .method_router
                        .route_layer(middleware::from_fn_with_state(
                            (state.clone(), permission),
                            enforce_permission,
                        ))
                }
                None => route.method_router,
            };
            router = router.route(&route.path, method_router);
        }

        // `Router::layer` runs on matched routes, `.fallback(...)`, and
        // `.method_not_allowed_fallback(...)`, ensuring every unauthenticated
        // compat request receives 401 before reaching any handler or fallback.
        let inner = router
            .fallback(unimplemented_fallback)
            .method_not_allowed_fallback(unimplemented_fallback)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                require_api_key,
            ))
            .with_state(state.clone());

        CompatService {
            base: self.base,
            inner,
        }
    }

    #[cfg(test)]
    fn raw_route(
        mut self,
        method: Method,
        path: &str,
        method_router: MethodRouter<AppState>,
    ) -> Self {
        let normalized = normalize_route_path(path);
        let entry_path = join_base(&self.base, &normalized);
        self.entries.push(CompatRouteEntry {
            method,
            path: entry_path,
            permission: None,
        });
        self.routes.push(RegisteredCompatRoute {
            path: normalized,
            permission: None,
            method_router,
        });
        self
    }
}

/// Verifies a slice of [`CompatRouteEntry`] values for missing permissions and
/// duplicate registrations.
pub fn verify_entries(entries: &[CompatRouteEntry]) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut seen: BTreeMap<(String, String), Option<Permission>> = BTreeMap::new();

    for entry in entries {
        let key = (
            entry.method.as_str().to_owned(),
            canonical_route_path(&entry.path),
        );
        if seen.insert(key, entry.permission).is_some() {
            errors.push(format!(
                "{} {}: duplicate compat route registration",
                entry.method, entry.path
            ));
        }
        if entry.permission.is_none() {
            errors.push(format!(
                "{} {}: missing permission declaration",
                entry.method, entry.path
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Tower [`Service`] wrapping the compat [`Router`] so URI paths are lowercased
/// before Axum routes them, while keeping query strings intact and preserving
/// the caller's original path for the 501 fallback.
#[derive(Clone)]
pub struct CompatService {
    base: String,
    inner: Router,
}

impl Service<Request> for CompatService {
    type Response = Response;
    type Error = Infallible;
    type Future = <Router as Service<Request>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <Router as Service<Request>>::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let original_path = req
            .extensions()
            .get::<OriginalUri>()
            .map(|uri| uri.0.path().to_owned())
            .unwrap_or_else(|| {
                let path = req.uri().path();
                if self.base.is_empty() || path.starts_with(&self.base) {
                    path.to_owned()
                } else {
                    join_base(&self.base, path)
                }
            });
        req.extensions_mut()
            .insert(OriginalCompatPath(original_path));

        let normalized = lowercase_uri_path(req.uri());
        *req.uri_mut() = normalized;

        self.inner.call(req)
    }
}

/// Matches `prefix` against `req.uri().path()` ASCII case-insensitively and, on
/// match, records [`OriginalUri`] and strips the prefix so a nested
/// [`CompatService`] can handle requests with non-lowercase mount prefixes
/// (for example `/Radarr/api/v3/QualityProfile`).
pub fn dispatch_prefix(mut req: Request, prefix: &str) -> Result<Request, Request> {
    let Some(rest) = strip_prefix_case_insensitive(req.uri().path(), prefix) else {
        return Err(req);
    };
    let original = req.uri().clone();
    let path_and_query = match original.query() {
        Some(query) => format!("{rest}?{query}"),
        None => rest.to_owned(),
    };
    let mut parts = original.clone().into_parts();
    parts.path_and_query = path_and_query.parse().ok().or(parts.path_and_query);
    if let Ok(rewritten) = Uri::from_parts(parts) {
        req.extensions_mut().insert(OriginalUri(original));
        *req.uri_mut() = rewritten;
    }
    Ok(req)
}

fn strip_prefix_case_insensitive<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = path.get(prefix.len()..)?;
    if !path[..prefix.len()].eq_ignore_ascii_case(prefix) {
        return None;
    }
    if rest.is_empty() {
        Some("/")
    } else if rest.starts_with('/') {
        Some(rest)
    } else {
        None
    }
}

fn method_router_for<H, T>(method: &Method, handler: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    match *method {
        Method::GET => axum::routing::get(handler),
        Method::POST => axum::routing::post(handler),
        Method::PUT => axum::routing::put(handler),
        Method::DELETE => axum::routing::delete(handler),
        Method::PATCH => axum::routing::patch(handler),
        Method::HEAD => axum::routing::head(handler),
        Method::OPTIONS => axum::routing::options(handler),
        Method::TRACE => axum::routing::trace(handler),
        _ => panic!("unsupported HTTP method for compat route: {method}"),
    }
}

fn normalize_base(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(trimmed.len() + 1);
    if !trimmed.starts_with('/') {
        out.push('/');
    }
    out.push_str(&trimmed.to_ascii_lowercase());
    out
}

fn join_base(base: &str, path: &str) -> String {
    if base.is_empty() {
        path.to_owned()
    } else if path == "/" {
        base.to_owned()
    } else {
        format!("{base}{path}")
    }
}

/// Normalizes a compat route pattern: ensures a leading `/`, drops trailing
/// slashes (except `/`), and lowercases static segments while keeping `{param}`
/// placeholder names intact for Axum's `Path` extractor.
fn normalize_route_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    let mut out = String::with_capacity(trimmed.len() + 1);
    if !trimmed.starts_with('/') {
        out.push('/');
    }
    let mut in_param = false;
    for ch in trimmed.chars() {
        match ch {
            '{' => {
                in_param = true;
                out.push(ch);
            }
            '}' => {
                in_param = false;
                out.push(ch);
            }
            _ if in_param => out.push(ch),
            _ => out.push(ch.to_ascii_lowercase()),
        }
    }
    out
}

/// Canonical form used for duplicate detection: lowercased and with `{param}`
/// placeholders collapsed to `{}` so `/movie/{id}` and `/movie/{movieId}` are
/// recognized as the same route.
fn canonical_route_path(path: &str) -> String {
    let normalized = normalize_route_path(path);
    let mut out = String::with_capacity(normalized.len());
    let mut in_param = false;
    for ch in normalized.chars() {
        match ch {
            '{' => {
                in_param = true;
                out.push(ch);
            }
            '}' => {
                in_param = false;
                out.push(ch);
            }
            _ if in_param => {}
            _ => out.push(ch),
        }
    }
    out
}

/// Lowercases the URI path and removes trailing slashes (except `/`), leaving
/// the query string untouched so case-sensitive query values (like `apikey` or
/// search terms) are preserved.
fn lowercase_uri_path(uri: &Uri) -> Uri {
    let path = uri.path();
    let trimmed = path.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    let lower = trimmed.to_ascii_lowercase();
    if lower == path {
        return uri.clone();
    }
    let path_and_query = match uri.query() {
        Some(query) => format!("{lower}?{query}"),
        None => lower,
    };
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = path_and_query.parse().ok().or(parts.path_and_query);
    Uri::from_parts(parts).unwrap_or_else(|_| uri.clone())
}

#[cfg(test)]
mod tests {
    use axum::routing::get;

    use super::*;

    async fn ok() {}

    #[test]
    fn route_path_normalization_lowercases_static_segments_only() {
        assert_eq!(normalize_route_path("/qualityProfile"), "/qualityprofile");
        assert_eq!(normalize_route_path("/QualityProfile/"), "/qualityprofile");
        assert_eq!(normalize_route_path("System/Status"), "/system/status");
        assert_eq!(normalize_route_path("/Movie/{movieId}"), "/movie/{movieId}");
        assert_eq!(normalize_route_path("/"), "/");
        assert_eq!(normalize_route_path(""), "/");
    }

    #[test]
    fn lowercase_uri_path_preserves_query_casing() {
        let uri: Uri = "/QualityProfile/?apikey=AbCdEf&term=Tmdb:603"
            .parse()
            .unwrap();
        assert_eq!(
            lowercase_uri_path(&uri).to_string(),
            "/qualityprofile?apikey=AbCdEf&term=Tmdb:603"
        );
        let root: Uri = "///?apikey=Secret".parse().unwrap();
        assert_eq!(lowercase_uri_path(&root).to_string(), "/?apikey=Secret");
    }

    #[test]
    fn strip_prefix_matches_case_insensitively_on_segment_boundary() {
        assert_eq!(
            strip_prefix_case_insensitive("/Radarr/API/v3/QualityProfile", "/radarr/api/v3"),
            Some("/QualityProfile")
        );
        assert_eq!(
            strip_prefix_case_insensitive("/RADARR/api/v3", "/radarr/api/v3"),
            Some("/")
        );
        assert_eq!(
            strip_prefix_case_insensitive("/radarr/api/v30/movie", "/radarr/api/v3"),
            None
        );
        assert_eq!(
            strip_prefix_case_insensitive("/api/v1/movies", "/radarr/api/v3"),
            None
        );
    }

    #[test]
    fn verify_rejects_route_without_permission() {
        let router = CompatRouter::with_base("/radarr/api/v3").raw_route(
            Method::GET,
            "/qualityprofile",
            get(ok),
        );
        let errors = router.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("missing permission declaration")),
            "{errors:?}"
        );
    }

    #[test]
    fn verify_rejects_case_insensitive_and_param_duplicates() {
        let case_dup = CompatRouter::with_base("/radarr/api/v3")
            .require(Permission::ProfilesRead, Method::GET, "/qualityprofile", ok)
            .require(Permission::ProfilesRead, Method::GET, "/QualityProfile", ok);
        let errors = case_dup.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("duplicate compat route registration")),
            "{errors:?}"
        );

        let param_dup = CompatRouter::with_base("/radarr/api/v3")
            .require(Permission::LibraryRead, Method::GET, "/movie/{id}", ok)
            .require(Permission::LibraryRead, Method::GET, "/movie/{movieId}", ok);
        let errors = param_dup.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("duplicate compat route registration")),
            "{errors:?}"
        );
    }

    #[test]
    fn verify_allows_different_methods_on_same_path() {
        let router = CompatRouter::with_base("/radarr/api/v3")
            .require(Permission::LibraryRead, Method::GET, "/tag", ok)
            .require(Permission::LibraryEdit, Method::POST, "/tag", ok);
        assert_eq!(router.verify(), Ok(()));
        assert_eq!(router.entries().len(), 2);
    }
}
