//! The audit log: recording helper, sign-in events and the read route
//! (docs/11-security.md, "Audit log").
//!
//! `data` holds changed field names with old and new values. A secret is
//! never written, only the fact that it changed.

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, FromRequestParts, Query, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, SecondsFormat, Utc};
use db::audit::{Filter, NewEntry};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{Caller, Require, SystemAudit};
use crate::error::Problem;

const DEFAULT_PAGE_SIZE: u64 = 50;
const MAX_PAGE_SIZE: u64 = 500;
/// Sign-in bodies are small; a larger one is not read.
const BODY_LIMIT: usize = 64 * 1024;

/// The address of the peer that sent the request. `None` in tests that call
/// the router without a socket.
#[derive(Debug, Clone, Default)]
pub struct ClientIp(Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| address.ip().to_string()),
        ))
    }
}

/// Who acted.
#[derive(Debug, Clone)]
pub struct Actor {
    user_id: Option<String>,
    name: String,
    api_key: Option<i32>,
}

impl Actor {
    /// The user behind a request. The name is read now, so the row keeps it
    /// after the user is renamed or deleted.
    pub async fn of(state: &AppState, caller: &Caller) -> Self {
        let name = match db::users::find(&state.db, caller.user_id()).await {
            Ok(Some(detail)) => detail.user.name,
            _ => caller.user_id().to_owned(),
        };
        Self {
            user_id: Some(caller.user_id().to_owned()),
            name,
            api_key: caller.api_key(),
        }
    }

    pub fn user(id: &str, name: &str) -> Self {
        Self {
            user_id: Some(id.to_owned()),
            name: name.to_owned(),
            api_key: None,
        }
    }

    /// Somebody who is not a known user, for example a failed sign-in.
    pub fn unknown(name: &str) -> Self {
        Self {
            user_id: None,
            name: name.to_owned(),
            api_key: None,
        }
    }
}

/// One thing that happened.
#[derive(Debug)]
pub struct Event {
    action: &'static str,
    target: Option<(&'static str, String)>,
    data: Option<Value>,
}

impl Event {
    pub fn new(action: &'static str) -> Self {
        Self {
            action,
            target: None,
            data: None,
        }
    }

    pub fn target(mut self, kind: &'static str, id: impl ToString) -> Self {
        self.target = Some((kind, id.to_string()));
        self
    }

    pub fn data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Collects the fields that differ, as `{ field: { old, new } }`.
#[derive(Debug, Default)]
pub struct Changes(Map<String, Value>);

impl Changes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the field when `old` and `new` differ.
    pub fn field(mut self, name: &str, old: impl Serialize, new: impl Serialize) -> Self {
        let (old, new) = (json!(old), json!(new));
        if old != new {
            self.0
                .insert(name.into(), json!({ "old": old, "new": new }));
        }
        self
    }

    /// A secret that changed: the fact, not the value.
    pub fn secret(mut self, name: &str) -> Self {
        self.0.insert(name.into(), json!("changed"));
        self
    }

    pub fn into_data(self) -> Value {
        Value::Object(self.0)
    }
}

/// Writes one row. A failure is logged and does not undo the action that
/// already happened.
pub async fn record(state: &AppState, actor: &Actor, ip: &ClientIp, event: Event) {
    let (target_kind, target_id) = match event.target {
        Some((kind, id)) => (Some(kind.to_owned()), Some(id)),
        None => (None, None),
    };
    let entry = NewEntry {
        actor_user_id: actor.user_id.clone(),
        actor_name: actor.name.clone(),
        api_key_id: actor.api_key,
        action: event.action.to_owned(),
        target_kind,
        target_id,
        data: event.data,
        ip_address: ip.0.clone(),
    };
    if let Err(error) = db::audit::record(&state.db, entry).await {
        tracing::error!(%error, action = event.action, "audit log write failed");
    }
}

/// Records what the library's own routes do: sign-in, failed sign-in and
/// sign-out. Layered on the auth router only.
pub async fn auth_events(
    State(state): State<AppState>,
    ip: ClientIp,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().trim_end_matches('/').to_owned();
    if path.ends_with("/sign-in/email") {
        sign_in(&state, &ip, request, next).await
    } else if path.ends_with("/sign-out") {
        sign_out(&state, &ip, request, next).await
    } else {
        next.run(request).await
    }
}

async fn sign_in(state: &AppState, ip: &ClientIp, request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, BODY_LIMIT).await else {
        return next.run(Request::from_parts(parts, Body::empty())).await;
    };
    let email = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|body| body["email"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;

    let status = response.status();
    if status.is_success() {
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, BODY_LIMIT).await.unwrap_or_default();
        let reply: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if let Some(id) = reply["user"]["id"].as_str() {
            let name = reply["user"]["name"].as_str().unwrap_or(id);
            let event = Event::new("auth.sign_in").target("user", id);
            record(state, &Actor::user(id, name), ip, event).await;
        }
        return Response::from_parts(parts, Body::from(bytes));
    }
    if status.is_client_error() {
        let event = Event::new("auth.sign_in_failed").data(json!({ "email": email }));
        record(state, &Actor::unknown(&email), ip, event).await;
    }
    response
}

async fn sign_out(state: &AppState, ip: &ClientIp, request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    // Resolved before the session is deleted.
    let actor = match Caller::from_request_parts(&mut parts, state).await {
        Ok(caller) => Some(Actor::of(state, &caller).await),
        Err(_) => None,
    };
    let response = next.run(Request::from_parts(parts, body)).await;
    if let (Some(actor), true) = (actor, response.status().is_success()) {
        record(state, &actor, ip, Event::new("auth.sign_out")).await;
    }
    response
}

/// Query of `GET /api/v1/audit-log`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct AuditParams {
    /// User id of the actor.
    actor: Option<String>,
    /// For example `role.updated`.
    action: Option<String>,
    target_kind: Option<String>,
    target_id: Option<String>,
    /// RFC 3339. Rows at or after this time.
    from: Option<String>,
    /// RFC 3339. Rows before this time.
    to: Option<String>,
    /// Starts at 1.
    page: Option<u64>,
    /// 1 to 500. Default 50.
    page_size: Option<u64>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    id: i32,
    created_at: String,
    /// Empty after the actor is deleted; `actorName` still names them.
    actor_user_id: Option<String>,
    actor_name: String,
    api_key_id: Option<i32>,
    /// For example `role.updated`.
    action: String,
    target_kind: Option<String>,
    target_id: Option<String>,
    /// Changed fields with old and new values. Never a secret value.
    #[schema(value_type = Option<Object>)]
    data: Option<Value>,
    ip_address: Option<String>,
}

impl From<db::entities::audit_log::Model> for AuditEntry {
    fn from(row: db::entities::audit_log::Model) -> Self {
        Self {
            id: row.id,
            created_at: row.created_at,
            actor_user_id: row.actor_user_id,
            actor_name: row.actor_name,
            api_key_id: row.api_key_id,
            action: row.action,
            target_kind: row.target_kind,
            target_id: row.target_id,
            data: row.data,
            ip_address: row.ip_address,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    items: Vec<AuditEntry>,
    page: u64,
    page_size: u64,
    total: u64,
}

/// A time in the format the table stores, so that text comparison orders it.
fn stored_time(field: &str, text: Option<&str>) -> Result<Option<String>, Problem> {
    text.map(|text| {
        DateTime::parse_from_rfc3339(text)
            .map(|time| {
                time.with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true)
            })
            .map_err(|_| Problem::validation(field, "must be an RFC 3339 time"))
    })
    .transpose()
}

/// The audit log, newest first.
#[utoipa::path(
    get,
    path = "/api/v1/audit-log",
    operation_id = "list_audit_log",
    tag = "system",
    params(AuditParams),
    responses(
        (status = 200, description = "One page of rows", body = AuditPage),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn list(
    _: Require<SystemAudit>,
    State(state): State<AppState>,
    Query(params): Query<AuditParams>,
) -> Result<Json<AuditPage>, Problem> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if page == 0 {
        return Err(Problem::validation("page", "starts at 1"));
    }
    if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
        return Err(Problem::validation("pageSize", "must be 1 to 500"));
    }
    let filter = Filter {
        actor_user_id: params.actor,
        action: params.action,
        target_kind: params.target_kind,
        target_id: params.target_id,
        from: stored_time("from", params.from.as_deref())?,
        to: stored_time("to", params.to.as_deref())?,
    };
    let (rows, total) = db::audit::list(&state.db, &filter, page, page_size)
        .await
        .map_err(|error| {
            tracing::error!(%error, "audit log");
            Problem::internal()
        })?;
    Ok(Json(AuditPage {
        items: rows.into_iter().map(AuditEntry::from).collect(),
        page,
        page_size,
        total,
    }))
}
