//! Spike for issue #3 / ADR 0006. Throwaway code.
#[rustfmt::skip]
mod auth_schema;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use auth_schema::{AppAuthSchema, AppPluginSchema, create_auth_tables, user};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use better_auth::integrations::axum::{AxumIntegration, CurrentSession};
use better_auth::plugins::endpoint_context::EndpointContext;
use better_auth::plugins::user_admission::{
    UserValidationAction, UserValidationData, UserValidationRejection, ValidateUserInfo,
};
use better_auth::plugins::{ApiKeyPlugin, EmailPasswordPlugin, SessionManagementPlugin};
use better_auth::seaorm::sea_orm::{EntityTrait, PaginatorTrait};
use better_auth::seaorm::{
    Database, DatabaseHookUpdate, SeaOrmHookContext, SeaOrmHooks, SeaOrmStore,
};
use better_auth::server_api::{CreateKeyOptions, VerifyKeyOptions};
use better_auth::{AuthConfig, AuthResult, BetterAuth, FieldMap, database_hooks};
use serde_json::{Value, json};

type Auth = Arc<BetterAuth<AppAuthSchema>>;
type Disabled = Arc<Mutex<HashSet<String>>>;

/// Checks 3 and 4: runtime admission policy.
///
/// The callback runs inside the sign-up transaction. SeaORM's SQLite pool has
/// one connection by default, so a query on our own `DatabaseConnection` here
/// times out. The open/closed state is therefore kept in memory.
struct Admission {
    signup_open: AtomicBool,
}

/// Check 4: no new session for a disabled user, whatever the sign-in method.
struct BlockDisabled(Disabled);

#[database_hooks]
impl SeaOrmHooks<AppAuthSchema> for BlockDisabled {
    async fn before_create_session(
        &self,
        session: &mut FieldMap,
        _ctx: &SeaOrmHookContext<'_, AppAuthSchema>,
    ) -> AuthResult<DatabaseHookUpdate<FieldMap>> {
        let user_id = session
            .get("userId")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok(if self.0.lock().unwrap().contains(user_id) {
            DatabaseHookUpdate::Cancel
        } else {
            DatabaseHookUpdate::Continue
        })
    }
}

#[async_trait]
impl ValidateUserInfo<AppAuthSchema> for Admission {
    async fn validate(
        &self,
        data: &UserValidationData,
        _context: &EndpointContext<'_, AppAuthSchema>,
    ) -> AuthResult<Option<UserValidationRejection>> {
        match data.source.action {
            // Check 3: sign-up is open only while the user table is empty.
            UserValidationAction::CreateUser => {
                let admitted = self.signup_open.swap(false, Ordering::SeqCst);
                Ok((!admitted).then(|| UserValidationRejection::new("signup_closed")))
            }
            // SignIn is called for OAuth only, not for password sign-in.
            UserValidationAction::SignIn | UserValidationAction::LinkAccount => Ok(None),
        }
    }
}

#[derive(Clone)]
struct AppState {
    auth: Auth,
    disabled: Disabled,
}

impl axum::extract::FromRef<AppState> for Auth {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").unwrap_or("sqlite://spike.db?mode=rwc".into());
    let db = Database::connect(&url).await?;
    create_auth_tables(&db).await?;

    let config = AuthConfig::new("spike-secret-spike-secret-spike-secret-0123456789")
        .base_url("http://localhost:7979/api/auth");
    let store = SeaOrmStore::<AppAuthSchema>::new(config.clone(), db.clone());
    let disabled = Disabled::default();
    let store = store
        .with_plugin_schema::<AppPluginSchema>()
        .hook(BlockDisabled(disabled.clone()));
    let signup_open = AtomicBool::new(user::Entity::find().count(&db).await? == 0);

    let auth: Auth = Arc::new(
        BetterAuth::<AppAuthSchema>::new(config)
            .store(store)
            .validate_user_info(Arc::new(Admission { signup_open }))
            .plugin(EmailPasswordPlugin::new().enable_signup(true))
            .plugin(SessionManagementPlugin::new())
            .plugin(ApiKeyPlugin::builder().build())
            .build()
            .await?,
    );
    println!("plugins: {:?}", auth.plugin_names());

    let state = AppState {
        auth: auth.clone(),
        disabled,
    };
    let app = Router::new()
        .route("/spike/me", get(me))
        .route("/spike/disable/{user_id}", post(disable))
        .route("/spike/apikey/{user_id}", post(create_key))
        .route("/radarr/api/v3/system/status", get(compat))
        .nest("/api/auth", auth.axum_router_with_state::<AppState>())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:7979").await?;
    println!("listening on 7979");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn me(session: CurrentSession<AppAuthSchema>) -> Json<Value> {
    Json(json!({ "id": session.user.id, "email": session.user.email }))
}

/// Check 4: revoke every session of another user from server code.
async fn disable(State(state): State<AppState>, Path(user_id): Path<String>) -> Json<Value> {
    state.disabled.lock().unwrap().insert(user_id.clone());
    let revoked = state
        .auth
        .session_manager()
        .revoke_all_user_sessions(&user_id)
        .await;
    Json(json!({ "revoked": revoked.map_err(|e| e.to_string()) }))
}

/// Check 5: issue a key from server code.
async fn create_key(State(state): State<AppState>, Path(user_id): Path<String>) -> Json<Value> {
    let created = state
        .auth
        .api_keys()
        .unwrap()
        .create(&user_id, CreateKeyOptions::default())
        .await;
    Json(json!({ "key": created.map(|c| c.key).map_err(|e| e.to_string()) }))
}

/// Check 5: authenticate a compat request by `X-Api-Key` or `?apikey=`.
async fn compat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or(query.get("apikey").map(String::as_str));
    let Some(key) = key else {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "no key" })));
    };
    match state
        .auth
        .api_keys()
        .unwrap()
        .verify(key, VerifyKeyOptions::default())
        .await
    {
        Ok(view) => (
            StatusCode::OK,
            Json(json!({ "apiKey": serde_json::to_value(&view).unwrap_or_default() })),
        ),
        Err(error) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": format!("{error:?}") })),
        ),
    }
}
