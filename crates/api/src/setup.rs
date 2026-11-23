//! First-run Owner setup (docs/15-permissions.md, "First run").

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use better_auth::integrations::axum::AxumIntegration;
use better_auth::seaorm::sea_orm::{EntityTrait, PaginatorTrait};
use db::DatabaseConnection;
use db::auth_schema::user;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower::ServiceExt;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::Public;
use crate::error::Problem;

/// Whether the first user still has to be created. Once a user exists the
/// answer stays "no": the last Owner can never be deleted.
#[derive(Clone)]
pub struct Setup {
    required: Arc<AtomicBool>,
    /// Serializes setup calls, so that two of them cannot both see an empty
    /// `user` table.
    lock: Arc<Mutex<()>>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            required: Arc::new(AtomicBool::new(true)),
            lock: Arc::default(),
        }
    }
}

impl Setup {
    /// Reads the database until it finds a user, then only the flag.
    pub async fn required(&self, db: &DatabaseConnection) -> Result<bool, Problem> {
        if !self.required.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let users = user::Entity::find().count(db).await.map_err(|error| {
            tracing::error!(%error, "count users");
            Problem::internal()
        })?;
        if users > 0 {
            self.required.store(false, Ordering::SeqCst);
        }
        Ok(users == 0)
    }
}

/// While no user exists, every API route except these answers 503
/// `setup_required`: our API, the auth routes and the compat prefixes
/// (`/radarr/api/...`). Static files have no `/api/` in their path and stay
/// served.
const OPEN_DURING_SETUP: [&str; 2] = ["/api/v1/setup", "/api/v1/health"];

pub async fn gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if path.contains("/api/") && !OPEN_DURING_SETUP.contains(&path) {
        match state.setup.required(&state.db).await {
            Ok(false) => {}
            Ok(true) => return setup_required().into_response(),
            Err(problem) => return problem.into_response(),
        }
    }
    next.run(request).await
}

fn setup_required() -> Problem {
    let mut problem = Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "setup_required",
        "Setup required",
    );
    problem.detail = Some("create the Owner account first".into());
    problem
}

#[derive(Serialize, ToSchema)]
pub struct SetupStatus {
    /// True until the first user exists.
    required: bool,
}

/// Whether the first-run setup is still required. Public.
#[utoipa::path(
    get,
    path = "/api/v1/setup",
    tag = "system",
    responses((status = 200, description = "Setup status", body = SetupStatus))
)]
pub async fn status(
    _: Public,
    State(state): State<AppState>,
) -> Result<Json<SetupStatus>, Problem> {
    let required = state.setup.required(&state.db).await?;
    Ok(Json(SetupStatus { required }))
}

#[derive(Deserialize, ToSchema)]
pub struct SetupRequest {
    name: String,
    email: String,
    /// At least 10 characters.
    password: String,
}

#[derive(Serialize, ToSchema)]
pub struct SetupUser {
    id: String,
    name: String,
    email: String,
}

/// Creates the first user with the Owner role and signs it in. Public while
/// no user exists, 409 after.
#[utoipa::path(
    post,
    path = "/api/v1/setup",
    tag = "system",
    request_body = SetupRequest,
    responses(
        (status = 201, description = "The Owner exists and is signed in; the response sets the session cookie", body = SetupUser),
        (status = 409, description = "A user already exists", body = Problem),
        (status = 422, description = "The library refused the name, email or password", body = Problem),
    )
)]
pub async fn create(
    _: Public,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> Result<Response, Problem> {
    let _guard = state.setup.lock.lock().await;
    if !state.setup.required(&state.db).await? {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "setup_done",
            "Setup is already done",
        ));
    }

    // The library's own sign-up, so that hashing, validation and the session
    // cookie are the library's. `Policy` assigns the Owner role inside its
    // transaction.
    let mut request = Request::post("/api/auth/sign-up/email")
        .body(Body::from(
            json!({ "name": body.name, "email": body.email, "password": body.password })
                .to_string(),
        ))
        .map_err(|_| Problem::internal())?;
    *request.headers_mut() = headers;
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    request.headers_mut().remove(header::CONTENT_LENGTH);

    state.policy.begin_owner_setup();
    let response = state
        .auth
        .clone()
        .axum_router_with_state::<AppState>()
        .with_state(state.clone())
        .oneshot(request)
        .await;
    state.policy.end_owner_setup();
    let response = response.map_err(|_| Problem::internal())?;

    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1 << 20)
        .await
        .map_err(|_| Problem::internal())?;
    let reply: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !parts.status.is_success() {
        if parts.status.is_server_error() {
            tracing::error!(status = %parts.status, %reply, "setup sign-up failed");
            return Err(Problem::internal());
        }
        let mut problem = Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation",
            "Validation failed",
        );
        // The library's message, for example "Password too short".
        problem.detail = reply["message"].as_str().map(str::to_owned);
        return Err(problem);
    }

    // The user exists now; the flag no longer needs the database.
    state.setup.required(&state.db).await?;
    tracing::info!("first-run setup created the Owner");
    let created = &reply["user"];
    let field = |name: &str| created[name].as_str().unwrap_or_default().to_owned();
    let user = SetupUser {
        id: field("id"),
        name: field("name"),
        email: field("email"),
    };
    let mut response = (StatusCode::CREATED, Json(user)).into_response();
    for cookie in parts.headers.get_all(header::SET_COOKIE) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.clone());
    }
    Ok(response)
}
