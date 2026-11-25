//! HTTP API: routes, the OpenAPI document and the Axum router.

pub mod access;
mod api_keys;
pub mod audit;
pub mod auth;
mod collections;
mod download_clients;
pub mod error;
mod health;
mod indexers;
mod library;
mod lookup;
mod me;
mod movies;
mod owner;
mod profiles;
mod proxy;
mod quality_definitions;
mod roles;
mod root_folders;
pub mod setup;
mod tags;
mod tmdb;
mod users;
mod web;

use axum::Router;
use axum::extract::FromRef;
use better_auth::integrations::axum::AxumIntegration;
use better_auth::{AuthError, AuthResult};
use db::DatabaseConnection;
use unseen_core::permission::Permission;
use utoipa::OpenApi;
use utoipa_axum::routes;

use crate::access::ApiRouter;
use crate::error::Problem;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub auth: auth::Auth,
    pub policy: auth::Policy,
    pub setup: setup::Setup,
    /// Encrypts the integration secrets (ADR 0011).
    pub secrets: db::secret::SecretKey,
    /// The client for TMDB and the other metadata calls.
    pub http: unseen_core::http::OutboundClient,
    /// Tests point this at a mock server.
    pub tmdb_base_url: String,
    /// Settings given as `UNSEEN_SETTING_<KEY>`: they win over the table and
    /// cannot be changed through the API (docs/10-configuration.md).
    pub settings_overrides: db::settings::Overrides,
}

impl AppState {
    /// `db` must be migrated: the auth instance reads its tables.
    pub async fn new(
        db: DatabaseConnection,
        secret: &str,
        base_url: Option<&str>,
    ) -> AuthResult<Self> {
        let (auth, policy) = auth::build(db.clone(), secret, base_url).await?;
        let disabled = db::users::disabled_ids(&db)
            .await
            .map_err(|error| AuthError::internal(format!("load disabled users: {error}")))?;
        for id in &disabled {
            policy.disable(id);
        }
        Ok(Self {
            db,
            auth,
            policy,
            setup: setup::Setup::default(),
            secrets: db::secret::SecretKey::derive(secret),
            http: unseen_core::http::OutboundClient::new()
                .map_err(|error| AuthError::internal(format!("build http client: {error}")))?,
            tmdb_base_url: unseen_core::tmdb::DEFAULT_BASE_URL.to_owned(),
            settings_overrides: db::settings::Overrides::from_env(),
        })
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

/// Every API route is registered here once with its access rule, so the
/// router, runtime enforcement and `x-permission` in OpenAPI cannot disagree.
pub fn routes() -> ApiRouter {
    ApiRouter::with_openapi(ApiDoc::openapi())
        .public(routes!(health::health))
        .public(routes!(setup::status, setup::create))
        .authenticated(routes!(me::me))
        .authenticated(routes!(owner::transfer))
        .authenticated(routes!(roles::list_permissions))
        .authenticated(routes!(api_keys::list_own, api_keys::create_own))
        .authenticated(routes!(api_keys::revoke_own))
        .require(Permission::ApikeysManage, routes!(api_keys::list_all))
        .require(Permission::ApikeysManage, routes!(api_keys::revoke_any))
        .require(
            Permission::UsersManage,
            routes!(roles::list_assignable_roles),
        )
        .require(
            Permission::RolesManage,
            routes!(roles::list_roles, roles::create_role),
        )
        .require(
            Permission::RolesManage,
            routes!(roles::update_role, roles::delete_role),
        )
        .require(
            Permission::UsersManage,
            routes!(users::list_users, users::create_user),
        )
        .require(
            Permission::UsersManage,
            routes!(users::update_user, users::delete_user),
        )
        .require(
            Permission::UsersManage,
            routes!(users::set_user_permissions),
        )
        .require(Permission::SystemAudit, routes!(audit::list))
        .require(Permission::ProfilesRead, routes!(profiles::list_profiles))
        .require(
            Permission::ProfilesManage,
            routes!(profiles::create_profile),
        )
        .require(Permission::ProfilesRead, routes!(profiles::get_profile))
        .require(
            Permission::ProfilesManage,
            routes!(profiles::update_profile, profiles::delete_profile),
        )
        .require(Permission::ImportRun, routes!(library::scan))
        .require(Permission::ImportRun, routes!(library::adopt))
        .require(Permission::LibraryAdd, routes!(lookup::lookup))
        .require(Permission::LibraryRead, routes!(movies::list_movies))
        .require(Permission::LibraryAdd, routes!(movies::create_movie))
        .require(Permission::LibraryRead, routes!(movies::get_movie))
        .require(Permission::LibraryEdit, routes!(movies::update_movie))
        .require(Permission::LibraryDelete, routes!(movies::delete_movie))
        .require(Permission::LibraryAdd, routes!(movies::add_profile))
        .require(Permission::LibraryDelete, routes!(movies::remove_profile))
        .require(Permission::LibraryRead, routes!(movies::list_files))
        .require(Permission::LibraryDelete, routes!(movies::delete_file))
        .require(
            Permission::LibraryRead,
            routes!(collections::list_collections),
        )
        .require(
            Permission::LibraryRead,
            routes!(collections::get_collection),
        )
        .require(
            Permission::LibraryEdit,
            routes!(collections::update_collection),
        )
        .require(Permission::LibraryRead, routes!(tags::list_tags))
        .require(
            Permission::LibraryEdit,
            routes!(tags::create_tag, tags::update_tag, tags::delete_tag),
        )
        .require(
            Permission::ProfilesRead,
            routes!(quality_definitions::list_definitions),
        )
        .require(
            Permission::ProfilesManage,
            routes!(quality_definitions::update_definition),
        )
        .require(Permission::SettingsRead, routes!(tmdb::get_settings))
        .require(Permission::SettingsManage, routes!(tmdb::update_settings))
        .require(Permission::SettingsRead, routes!(proxy::get_settings))
        .require(
            Permission::SettingsManage,
            routes!(proxy::update_settings, proxy::test_proxy),
        )
        .require(
            Permission::SettingsRead,
            routes!(root_folders::list_root_folders),
        )
        .require(
            Permission::SettingsManage,
            routes!(root_folders::create_root_folder),
        )
        .require(
            Permission::SettingsManage,
            routes!(root_folders::delete_root_folder),
        )
        .require(Permission::SettingsManage, routes!(root_folders::browse))
        .require(Permission::IndexersRead, routes!(indexers::list_indexers))
        .require(
            Permission::IndexersManage,
            routes!(indexers::create_indexer),
        )
        .require(Permission::IndexersRead, routes!(indexers::get_indexer))
        .require(
            Permission::IndexersManage,
            routes!(
                indexers::update_indexer,
                indexers::delete_indexer,
                indexers::test_indexer
            ),
        )
        .require(
            Permission::ClientsRead,
            routes!(download_clients::list_clients),
        )
        .require(
            Permission::ClientsManage,
            routes!(download_clients::create_client),
        )
        .require(
            Permission::ClientsManage,
            routes!(
                download_clients::update_client,
                download_clients::delete_client,
                download_clients::test_client
            ),
        )
}

/// The OpenAPI document. `just gen` writes it to `openapi.json`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().into_openapi()
}

pub fn router(state: AppState) -> Router {
    let api = routes().into_router(&state);
    // The auth routes are the library's own; they are not in our OpenAPI
    // document.
    let auth =
        state
            .auth
            .clone()
            .axum_router_with_state()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                audit::auth_events,
            ));
    api.nest("/api/auth", auth)
        .fallback(web::serve)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            setup::gate,
        ))
        .with_state(state)
}
