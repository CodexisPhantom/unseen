//! Server-sent events (`GET /api/v1/events`) with permission filtering at send
//! time (docs/05-api.md, "SSE").

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time::Instant;
use unseen_core::permission::{EffectivePermissions, Permission};

use crate::AppState;
use crate::access::Caller;
use crate::auth::Policy;
use crate::error::Problem;

/// One event published on the bus. `permission` is checked against the
/// recipient's live grants when the event is sent; `target_user` limits an
/// event to one user.
#[derive(Debug, Clone)]
pub struct AppEvent {
    pub kind: String,
    pub data: Value,
    pub permission: Option<Permission>,
    pub target_user: Option<String>,
}

/// Broadcast bus shared by every connected SSE client.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// The channel holds 256 events; a client that falls further behind
    /// resynchronizes its permissions and continues with new events.
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }

    /// Sends `event` to every listener. A no-op when nobody is connected.
    pub fn publish(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }
}

struct FilterState {
    rx: broadcast::Receiver<AppEvent>,
    db: db::DatabaseConnection,
    policy: Policy,
    user_id: String,
    api_key: Option<i32>,
    session_id: Option<String>,
    permissions: Option<EffectivePermissions>,
}

impl FilterState {
    /// Reloads the live grants. Returns false when the account can no longer
    /// receive events at all.
    async fn refresh(&mut self) -> bool {
        match db::access::grants_of_user(&self.db, &self.user_id).await {
            Ok(grants) => {
                self.permissions = Some(EffectivePermissions::new(
                    grants.is_owner,
                    &grants.permissions,
                ));
                true
            }
            Err(error) => {
                tracing::error!(%error, user = %self.user_id, "failed to load SSE permissions");
                false
            }
        }
    }

    /// Checks that the identity behind the stream is still valid right now:
    /// the account is enabled and the API key, when one authenticated the
    /// stream, is still live and still owned by the same user.
    async fn revoked(&self) -> bool {
        if self.policy.is_disabled(&self.user_id) {
            return true;
        }
        if let Some(id) = &self.session_id {
            use better_auth::seaorm::sea_orm::EntityTrait;

            match db::auth_schema::session::Entity::find_by_id(id)
                .one(&self.db)
                .await
            {
                Ok(Some(session)) if session.user_id == self.user_id => {}
                Ok(_) => return true,
                Err(error) => {
                    tracing::error!(%error, session = %id, "failed to load SSE session");
                    return true;
                }
            }
        }
        let Some(id) = self.api_key else {
            return false;
        };
        match db::api_keys::find(&self.db, id).await {
            Ok(Some(key)) => key.user_id != self.user_id,
            Ok(None) => true,
            Err(error) => {
                tracing::error!(%error, key = id, "failed to load SSE API key");
                true
            }
        }
    }

    /// The permissions the API key narrows the user's grants to, if any.
    async fn key_permissions(&self) -> Option<BTreeSet<String>> {
        let id = self.api_key?;
        match db::api_keys::find(&self.db, id).await {
            Ok(Some(key)) => Some(key.permissions),
            Ok(None) => Some(BTreeSet::new()),
            Err(error) => {
                tracing::error!(%error, key = id, "failed to load SSE API key");
                Some(BTreeSet::new())
            }
        }
    }
}

/// A stream of events the caller may see. Permissions are loaded lazily and
/// refreshed after a lag or a `permissions.changed` event, so a revoked
/// permission stops delivery on an already open connection.
pub fn filtered(
    bus: &EventBus,
    db: db::DatabaseConnection,
    policy: Policy,
    user_id: String,
    api_key: Option<i32>,
    session_id: Option<String>,
) -> impl Stream<Item = Result<Event, Infallible>> + use<> {
    let state = FilterState {
        rx: bus.subscribe(),
        db,
        policy,
        user_id,
        api_key,
        session_id,
        permissions: None,
    };
    stream::unfold(state, |mut state| async move {
        let mut session_check = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(15),
            Duration::from_secs(15),
        );
        loop {
            let received = tokio::select! {
                _ = session_check.tick(), if state.session_id.is_some() => {
                    if state.revoked().await {
                        return None;
                    }
                    continue;
                }
                received = state.rx.recv() => received,
            };
            match received {
                Ok(event) => {
                    if state.revoked().await {
                        return None;
                    }
                    if event.kind == "permissions.changed" && !state.refresh().await {
                        return None;
                    }
                    if let Some(target) = &event.target_user
                        && target != &state.user_id
                    {
                        continue;
                    }
                    if let Some(permission) = event.permission {
                        if state.permissions.is_none() && !state.refresh().await {
                            return None;
                        }
                        let Some(permissions) = &state.permissions else {
                            continue;
                        };
                        if !permissions.has(permission) {
                            continue;
                        }
                        if let Some(key) = state.key_permissions().await
                            && !key.contains(permission.as_str())
                        {
                            continue;
                        }
                    }
                    match Event::default()
                        .event(event.kind.as_str())
                        .json_data(event.data)
                    {
                        Ok(sse) => return Some((Ok(sse), state)),
                        Err(error) => {
                            tracing::error!(%error, kind = %event.kind, "failed to encode SSE event");
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        user = %state.user_id,
                        skipped,
                        "SSE client lagged behind event bus"
                    );
                    if !state.refresh().await {
                        return None;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// `GET /api/v1/events`: the caller's permission-filtered event stream.
#[utoipa::path(
    get,
    path = "/api/v1/events",
    tag = "events",
    responses(
        (
            status = 200,
            description = "Server-sent event stream",
            content_type = "text/event-stream",
            body = String,
        ),
        (status = 401, description = "Not signed in", body = Problem),
    )
)]
pub async fn stream_events(
    State(state): State<AppState>,
    caller: Caller,
) -> axum::response::Response {
    let stream = filtered(
        &state.events,
        state.db.clone(),
        state.policy.clone(),
        caller.user_id().to_owned(),
        caller.api_key(),
        caller.session_id().map(str::to_owned),
    );
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response();
    response.headers_mut().insert(
        "x-accel-buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    response
}
