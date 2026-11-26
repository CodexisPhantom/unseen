//! Request authentication, permission enforcement and OpenAPI `x-permission`.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::marker::PhantomData;
use std::ops::Deref;

use axum::Router;
use axum::extract::{FromRequestParts, Query, Request, State};
use axum::http::Method;
use axum::http::request::Parts;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use better_auth::integrations::axum::OptionalSession;
use db::auth_schema::AppAuthSchema;
use unseen_core::permission::{EffectivePermissions, Permission};
use utoipa::openapi::extensions::Extensions;
use utoipa::openapi::path::{Operation, PathItem};
use utoipa::openapi::{Components, OpenApi};
use utoipa_axum::router::UtoipaMethodRouter;

use crate::AppState;
use crate::api_keys;
use crate::error::Problem;

const API_KEY_HEADER: &str = "x-api-key";
const API_KEY_QUERY: &str = "apikey";

/// OpenAPI operation extension carrying the required permission, `"authenticated"`,
/// or `"public"`.
pub const X_PERMISSION: &str = "x-permission";

/// Access rule declared for a route and mirrored into `x-permission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Public,
    Authenticated,
    Permission(Permission),
}

impl Access {
    /// Wire value written to `x-permission` in OpenAPI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Authenticated => "authenticated",
            Self::Permission(permission) => permission.as_str(),
        }
    }
}

/// Marker extractor declaring that a handler is intentionally public.
#[derive(Debug, Clone, Copy, Default)]
pub struct Public;

impl<S: Send + Sync> FromRequestParts<S> for Public {
    type Rejection = Infallible;

    async fn from_request_parts(_parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self)
    }
}

/// Authenticated caller with permissions loaded from SQLite for this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    user_id: String,
    permissions: EffectivePermissions,
    /// The API key that authenticated the request, if it was not a session.
    api_key: Option<i32>,
    /// The better-auth session that authenticated the request, if any.
    session_id: Option<String>,
}

impl Caller {
    #[must_use]
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// The id of the API key behind this request, `None` for a session.
    #[must_use]
    pub fn api_key(&self) -> Option<i32> {
        self.api_key
    }

    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    #[must_use]
    pub fn permissions(&self) -> &EffectivePermissions {
        &self.permissions
    }

    #[must_use]
    pub fn has(&self, permission: Permission) -> bool {
        self.permissions.has(permission)
    }

    #[must_use]
    pub fn is_owner(&self) -> bool {
        self.permissions.is_owner()
    }
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = Problem;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Cache per request only so middleware and extractors share one DB load
        // without ever persisting permissions in the session (ADR 0007).
        if let Some(caller) = parts.extensions.get::<Self>() {
            return Ok(caller.clone());
        }

        let caller = match presented_api_key(parts) {
            Some(key) => from_api_key(state, &key).await?,
            None => from_session(parts, state).await?,
        };
        parts.extensions.insert(caller.clone());
        Ok(caller)
    }
}

/// Failure reason when resolving an API key.
pub(crate) enum ApiKeyRejection {
    Unauthorized,
    Internal,
}

/// The API key sent as `X-Api-Key` or `?apikey=` (matching the query parameter
/// name case-insensitively).
pub(crate) fn presented_api_key(parts: &Parts) -> Option<String> {
    if let Some(value) = parts.headers.get(API_KEY_HEADER) {
        return Some(value.to_str().unwrap_or_default().to_owned());
    }
    parts.uri.query()?;
    Query::<Vec<(String, String)>>::try_from_uri(&parts.uri)
        .ok()?
        .0
        .into_iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(API_KEY_QUERY))
        .map(|(_, value)| value)
}

/// Resolves an API key to a [`Caller`]. The key stands for its owner's
/// permissions it was given and the owner still holds, and never for the Owner
/// role.
pub(crate) async fn caller_from_api_key(
    state: &AppState,
    key: &str,
) -> Result<Caller, ApiKeyRejection> {
    let failed = |error: better_auth::seaorm::sea_orm::DbErr| {
        tracing::error!(%error, "failed to resolve API key");
        ApiKeyRejection::Internal
    };
    let owner = db::api_keys::authenticate(&state.db, &api_keys::hash(key))
        .await
        .map_err(failed)?
        .ok_or(ApiKeyRejection::Unauthorized)?;
    let grants = db::access::grants_of_user(&state.db, &owner.user_id)
        .await
        .map_err(failed)?;
    let user = EffectivePermissions::new(grants.is_owner, &grants.permissions);
    let narrowed = owner
        .permissions
        .iter()
        .filter_map(|name| name.parse::<Permission>().ok())
        .filter(|permission| user.has(*permission))
        .map(Permission::as_str);
    Ok(Caller {
        user_id: owner.user_id.clone(),
        permissions: EffectivePermissions::new(false, narrowed),
        api_key: Some(owner.key_id),
        session_id: None,
    })
}

/// A key that is present but not valid is refused: it never falls back to a
/// session cookie.
async fn from_api_key(state: &AppState, key: &str) -> Result<Caller, Problem> {
    caller_from_api_key(state, key)
        .await
        .map_err(|rejection| match rejection {
            ApiKeyRejection::Unauthorized => Problem::unauthorized(),
            ApiKeyRejection::Internal => Problem::internal(),
        })
}

async fn from_session(parts: &mut Parts, state: &AppState) -> Result<Caller, Problem> {
    let OptionalSession(Some(current)) =
        OptionalSession::<AppAuthSchema>::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                tracing::error!("session resolution failed");
                Problem::internal()
            })?
    else {
        return Err(Problem::unauthorized());
    };

    let Some(user_id) = current.user.id.as_str() else {
        tracing::error!("authenticated user has no string id");
        return Err(Problem::internal());
    };
    let Some(session_id) = current.session.id.as_str() else {
        tracing::error!("authenticated session has no string id");
        return Err(Problem::internal());
    };

    let grants = db::access::grants_of_user(&state.db, user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to load user permissions");
            Problem::internal()
        })?;

    Ok(Caller {
        user_id: user_id.to_owned(),
        permissions: EffectivePermissions::new(grants.is_owner, &grants.permissions),
        api_key: None,
        session_id: Some(session_id.to_owned()),
    })
}

/// Maps a zero-sized marker type to a [`Permission`] variant for [`Require`].
pub trait RequiredPermission: Send + Sync + 'static {
    const PERMISSION: Permission;
}

macro_rules! permission_markers {
    ($($variant:ident => $name:literal,)+) => {
        $(
            #[doc = concat!("Requires the `", $name, "` permission.")]
            #[derive(Debug, Clone, Copy, Default)]
            pub struct $variant;

            impl RequiredPermission for $variant {
                const PERMISSION: Permission = Permission::$variant;
            }
        )+
    };
}

unseen_core::for_each_permission!(permission_markers);

/// Extractor that authenticates the caller and requires permission `P`.
#[derive(Debug, Clone)]
pub struct Require<P: RequiredPermission>(Caller, PhantomData<P>);

impl<P: RequiredPermission> Require<P> {
    #[must_use]
    pub fn into_caller(self) -> Caller {
        self.0
    }
}

impl<P: RequiredPermission> Deref for Require<P> {
    type Target = Caller;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<P: RequiredPermission> FromRequestParts<AppState> for Require<P> {
    type Rejection = Problem;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let caller = Caller::from_request_parts(parts, state).await?;
        if !caller.has(P::PERMISSION) {
            return Err(Problem::forbidden(P::PERMISSION));
        }
        Ok(Self(caller, PhantomData))
    }
}

async fn enforce(
    State((state, access)): State<(AppState, Access)>,
    req: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    let caller = match Caller::from_request_parts(&mut parts, &state).await {
        Ok(caller) => caller,
        Err(problem) => return problem.into_response(),
    };
    if let Access::Permission(permission) = access
        && !caller.has(permission)
    {
        return Problem::forbidden(permission).into_response();
    }
    next.run(Request::from_parts(parts, body)).await
}

/// One registered HTTP operation and its declared access rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    pub method: Method,
    pub path: String,
    pub access: Option<Access>,
}

struct RegisteredRoute {
    path: String,
    access: Option<Access>,
    method_router: MethodRouter<AppState>,
}

/// Composes routes alongside the OpenAPI document so every route carries a
/// single access declaration enforced at runtime and mirrored to `x-permission`.
pub struct ApiRouter {
    openapi: OpenApi,
    routes: Vec<RegisteredRoute>,
    entries: Vec<RouteEntry>,
}

impl Default for ApiRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiRouter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_openapi(OpenApi::default())
    }

    #[must_use]
    pub fn with_openapi(openapi: OpenApi) -> Self {
        Self {
            openapi,
            routes: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Registers one or more public routes.
    #[must_use]
    pub fn public(self, route: UtoipaMethodRouter<AppState>) -> Self {
        self.route_with(Access::Public, route)
    }

    /// Registers routes requiring an authenticated user without a specific permission.
    #[must_use]
    pub fn authenticated(self, route: UtoipaMethodRouter<AppState>) -> Self {
        self.route_with(Access::Authenticated, route)
    }

    /// Registers routes requiring `permission`.
    #[must_use]
    pub fn require(self, permission: Permission, route: UtoipaMethodRouter<AppState>) -> Self {
        self.route_with(Access::Permission(permission), route)
    }

    /// Registers routes with the given [`Access`] declaration.
    #[must_use]
    pub fn route_with(self, access: Access, route: UtoipaMethodRouter<AppState>) -> Self {
        self.add_route(Some(access), route)
    }

    fn add_route(
        mut self,
        access: Option<Access>,
        (schemas, paths, method_router): UtoipaMethodRouter<AppState>,
    ) -> Self {
        let mut method_router = Some(method_router);
        let path_count = paths.paths.len();

        for (index, (path, mut path_item)) in paths.paths.into_iter().enumerate() {
            let normalized = if path.is_empty() {
                "/".to_owned()
            } else {
                path
            };

            for (method, operation) in path_item_operations_mut(&mut path_item) {
                if let Some(access) = access {
                    let extensions = operation.extensions.get_or_insert_with(Extensions::default);
                    // Preserve any manually declared `x-permission` so `verify()` catches disagreement.
                    extensions
                        .entry(X_PERMISSION.to_owned())
                        .or_insert_with(|| serde_json::json!(access.as_str()));
                }
                self.entries.push(RouteEntry {
                    method,
                    path: normalized.clone(),
                    access,
                });
            }

            if let Some(existing) = self.openapi.paths.paths.get_mut(&normalized) {
                existing.merge_operations(path_item);
            } else {
                self.openapi
                    .paths
                    .paths
                    .insert(normalized.clone(), path_item);
            }

            let router_for_path = if index + 1 == path_count {
                method_router
                    .take()
                    .expect("last path consumes method_router")
            } else {
                method_router
                    .as_ref()
                    .expect("method_router available before last path")
                    .clone()
            };

            self.routes.push(RegisteredRoute {
                path: normalized,
                access,
                method_router: router_for_path,
            });
        }

        let components = self.openapi.components.get_or_insert_with(Components::new);
        components.schemas.extend(schemas);
        self
    }

    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.openapi.merge(other.openapi);
        self.routes.extend(other.routes);
        self.entries.extend(other.entries);
        self
    }

    #[must_use]
    pub fn entries(&self) -> &[RouteEntry] {
        &self.entries
    }

    #[must_use]
    pub fn into_openapi(self) -> OpenApi {
        self.openapi
    }

    pub fn into_router(self, state: &AppState) -> Router<AppState> {
        let mut router = Router::new();
        for route in self.routes {
            // Use `route_layer` rather than `layer` so merging multiple HTTP methods
            // on the same path preserves Axum's default 405 Method Not Allowed fallback.
            let method_router = match route.access {
                Some(access @ (Access::Authenticated | Access::Permission(_))) => route
                    .method_router
                    .route_layer(middleware::from_fn_with_state(
                        (state.clone(), access),
                        enforce,
                    )),
                Some(Access::Public) | None => route.method_router,
            };
            router = router.route(&route.path, method_router);
        }
        router
    }

    /// Checks that every registered route declares an access rule, has no duplicates,
    /// and agrees with the OpenAPI document's paths and `x-permission` extensions.
    pub fn verify(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let mut router_ops: BTreeMap<(String, String), Option<Access>> = BTreeMap::new();

        for entry in &self.entries {
            let key = (entry.method.as_str().to_owned(), entry.path.clone());
            if router_ops.insert(key, entry.access).is_some() {
                errors.push(format!(
                    "{} {}: duplicate route registration",
                    entry.method, entry.path
                ));
            }
            if entry.access.is_none() {
                errors.push(format!(
                    "{} {}: missing access declaration",
                    entry.method, entry.path
                ));
            }
        }

        let mut openapi_ops = BTreeSet::new();
        for (path, item) in &self.openapi.paths.paths {
            for (method, operation) in openapi_path_item_operations(item) {
                let key = (method.clone(), path.clone());
                openapi_ops.insert(key.clone());

                match router_ops.get(&key) {
                    None => errors.push(format!(
                        "{method} {path}: present in OpenAPI but missing from router"
                    )),
                    Some(None) => {}
                    Some(Some(access)) => {
                        verify_operation_extension(&method, path, operation, *access, &mut errors);
                    }
                }
            }
        }

        for (method, path) in router_ops.keys() {
            if !openapi_ops.contains(&(method.clone(), path.clone())) {
                errors.push(format!(
                    "{method} {path}: registered on router but missing from OpenAPI"
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    #[cfg(test)]
    fn routes_without_access(self, route: UtoipaMethodRouter<AppState>) -> Self {
        self.add_route(None, route)
    }

    #[cfg(test)]
    fn raw_route(
        mut self,
        method: Method,
        path: &str,
        method_router: MethodRouter<AppState>,
    ) -> Self {
        self.entries.push(RouteEntry {
            method,
            path: path.to_owned(),
            access: None,
        });
        self.routes.push(RegisteredRoute {
            path: path.to_owned(),
            access: None,
            method_router,
        });
        self
    }

    #[cfg(test)]
    fn openapi_mut(&mut self) -> &mut OpenApi {
        &mut self.openapi
    }
}

fn verify_operation_extension(
    method: &str,
    path: &str,
    operation: &Operation,
    access: Access,
    errors: &mut Vec<String>,
) {
    let Some(value) = operation
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.get(X_PERMISSION))
    else {
        errors.push(format!(
            "{method} {path}: missing OpenAPI `{X_PERMISSION}` extension"
        ));
        return;
    };

    let Some(spec_perm) = value.as_str() else {
        errors.push(format!(
            "{method} {path}: OpenAPI `{X_PERMISSION}` must be a string"
        ));
        return;
    };

    let valid =
        matches!(spec_perm, "public" | "authenticated") || spec_perm.parse::<Permission>().is_ok();
    if !valid {
        errors.push(format!(
            "{method} {path}: invalid OpenAPI `{X_PERMISSION}` value `{spec_perm}`"
        ));
        return;
    }

    if spec_perm != access.as_str() {
        errors.push(format!(
            "{method} {path}: router declares `{}` but OpenAPI `{X_PERMISSION}` has `{spec_perm}`",
            access.as_str()
        ));
    }
}

fn path_item_operations_mut(item: &mut PathItem) -> Vec<(Method, &mut Operation)> {
    let slots = [
        (Method::GET, &mut item.get),
        (Method::PUT, &mut item.put),
        (Method::POST, &mut item.post),
        (Method::DELETE, &mut item.delete),
        (Method::OPTIONS, &mut item.options),
        (Method::HEAD, &mut item.head),
        (Method::PATCH, &mut item.patch),
        (Method::TRACE, &mut item.trace),
    ];
    slots
        .into_iter()
        .filter_map(|(method, op)| op.as_mut().map(|op| (method, op)))
        .collect()
}

fn openapi_path_item_operations(item: &PathItem) -> Vec<(String, &Operation)> {
    let slots = [
        ("GET", &item.get),
        ("PUT", &item.put),
        ("POST", &item.post),
        ("DELETE", &item.delete),
        ("OPTIONS", &item.options),
        ("HEAD", &item.head),
        ("PATCH", &item.patch),
        ("TRACE", &item.trace),
        ("QUERY", &item.query),
    ];
    let mut ops: Vec<(String, &Operation)> = slots
        .into_iter()
        .filter_map(|(method, op)| op.as_ref().map(|op| (method.to_owned(), op)))
        .collect();
    for (method, op) in &item.additional_operations {
        ops.push((method.to_ascii_uppercase(), op));
    }
    ops
}

#[cfg(test)]
mod tests {
    use axum::routing::get;
    use utoipa_axum::routes;

    use super::*;

    #[utoipa::path(get, path = "/api/v1/demo", responses((status = 200)))]
    async fn demo(_: Public) {}

    #[utoipa::path(get, path = "/api/v1/root", responses((status = 200)))]
    async fn root_path(_: Public) {}

    #[utoipa::path(
        get,
        path = "/api/v1/conflict",
        extensions(("x-permission" = json!("library:delete"))),
        responses((status = 200))
    )]
    async fn conflicting_extension() {}

    #[utoipa::path(
        get,
        path = "/api/v1/invalid-ext",
        extensions(("x-permission" = json!("library:fly"))),
        responses((status = 200))
    )]
    async fn invalid_extension() {}

    #[utoipa::path(
        get,
        path = "/api/v1/non-string-ext",
        extensions(("x-permission" = json!(42))),
        responses((status = 200))
    )]
    async fn non_string_extension() {}

    #[utoipa::path(get, path = "/api/v1/extra", responses((status = 200)))]
    async fn extra(_: Caller) {}

    #[test]
    fn access_as_str_matches_wire_format() {
        assert_eq!(Access::Public.as_str(), "public");
        assert_eq!(Access::Authenticated.as_str(), "authenticated");
        assert_eq!(
            Access::Permission(Permission::LibraryRead).as_str(),
            "library:read"
        );
    }

    #[test]
    fn verify_fails_for_route_without_access() {
        let router = ApiRouter::new().routes_without_access(routes!(demo));
        let errors = router.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("missing access declaration")),
            "{errors:?}"
        );
    }

    #[test]
    fn verify_fails_for_raw_route_missing_from_openapi() {
        let router = ApiRouter::new().raw_route(Method::GET, "/api/v1/raw", get(|| async {}));
        let errors = router.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("missing access declaration")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("missing from OpenAPI")),
            "{errors:?}"
        );
    }

    #[test]
    fn verify_fails_for_orphan_openapi_operation() {
        let mut router = ApiRouter::new().public(routes!(demo));
        router.openapi_mut().paths.paths.insert(
            "/api/v1/ghost".to_owned(),
            PathItem::new(utoipa::openapi::HttpMethod::Get, Operation::new()),
        );
        let errors = router.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("/api/v1/ghost")
                    && error.contains("missing from router")),
            "{errors:?}"
        );
    }

    #[test]
    fn verify_fails_when_openapi_extension_disagrees_or_is_missing_or_invalid() {
        let disagreement =
            ApiRouter::new().require(Permission::LibraryRead, routes!(conflicting_extension));
        let errors = disagreement.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("library:read") && error.contains("library:delete")),
            "{errors:?}"
        );

        let invalid = ApiRouter::new().require(Permission::LibraryRead, routes!(invalid_extension));
        let errors = invalid.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("invalid OpenAPI `x-permission` value `library:fly`")),
            "{errors:?}"
        );

        let non_string =
            ApiRouter::new().require(Permission::LibraryRead, routes!(non_string_extension));
        let errors = non_string.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("must be a string")),
            "{errors:?}"
        );

        let mut missing = ApiRouter::new().public(routes!(demo));
        missing
            .openapi_mut()
            .paths
            .paths
            .get_mut("/api/v1/demo")
            .unwrap()
            .get
            .as_mut()
            .unwrap()
            .extensions = None;
        let errors = missing.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("missing OpenAPI `x-permission` extension")),
            "{errors:?}"
        );
    }

    #[test]
    fn verify_fails_on_duplicate_route_registration() {
        let router = ApiRouter::new().public(routes!(demo)).public(routes!(demo));
        let errors = router.verify().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("duplicate route registration")),
            "{errors:?}"
        );
    }

    #[test]
    fn merge_combines_routes_and_passes_verify() {
        let first = ApiRouter::default().public(routes!(demo));
        let second = ApiRouter::new()
            .authenticated(routes!(extra))
            .public(routes!(root_path));
        let merged = first.merge(second);
        assert_eq!(merged.entries().len(), 3);
        assert_eq!(merged.verify(), Ok(()));
    }

    #[test]
    fn verify_detects_custom_openapi_methods() {
        let mut router = ApiRouter::new().public(routes!(demo));
        let item = router
            .openapi_mut()
            .paths
            .paths
            .get_mut("/api/v1/demo")
            .unwrap();
        item.query = Some(Operation::new());
        item.additional_operations
            .insert("mkcol".to_owned(), Operation::new());
        let errors = router.verify().unwrap_err();
        assert!(errors.iter().any(|error| error.starts_with("QUERY ")));
        assert!(errors.iter().any(|error| error.starts_with("MKCOL ")));
    }
}
