//! SSE transport for the `/ai/guide` advisor routes (issue #117, ADR-0013).
//!
//! A thin transport layer: it calls the public seam `ai_adapter::Guide::suggest`
//! and forwards its event stream to the frontend as an SSE stream, terminated
//! by `[DONE]`. It also owns `POST /ai/guide/{id}/interrupt`, which cancels the
//! upstream generation for the same session through a [`CancellationToken`].
//!
//! This module never reaches into `ai_adapter` internals and never writes to
//! the `Game` — it only takes a player-visible board snapshot (cloned under a
//! short lock) to hand to `suggest`, while the session registry pairs the
//! frontend-generated `sessionId` (the `{id}` path segment) with the cancel
//! token so a separate interrupt request can abort the analysis.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use futures::Stream;
use futures::StreamExt;
use futures::stream;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::ai::agent::AgentError;
use crate::ai::protocol::{ProviderError, ProviderErrorKind, StreamChunk};
use crate::ai_adapter::{GuideRequest, InterruptReason, SuggestPreFlightError};

use super::AppState;

/// The SSE wire events (issue #117). Tagged by `kind` so the frontend's
/// `GuideEvent(TS)` type is isomorphic on the wire:
/// `{kind:"reasoning",text}` / `{kind:"content",text}` /
/// `{kind:"interrupt",reason}`.
///
/// The variants carry an explicit `text` field (struct variants) because a
/// newtype variant like `Reasoning(String)` couldn't merge the `kind` tag into
/// a bare `String` under serde's internally-tagged representation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum GuideEventDto {
    Reasoning { text: String },
    Content { text: String },
    Interrupt { reason: InterruptReason },
}

/// Assembles the `/ai/...` routes onto a `Router`, given the `AppState`.
/// `server::routes` merges this into the game API router.
pub(crate) fn ai_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ai/guide/{id}", post(handle_guide))
        .route("/ai/guide/{id}/interrupt", post(handle_user_interrupt))
        .with_state(state)
}

/// `POST /ai/guide/{id}`: consumes the `Guide::suggest` event stream and
/// downstreams it as SSE. The `{id}` is the frontend-generated `sessionId`; it
/// is used only to associate this SSE's cancel token so the interrupt route can
/// abort the upstream generation.
async fn handle_guide(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<GuideRequest>,
) -> Response {
    // Register the cancel token before any stream work so a concurrent
    // `/interrupt` can find it. A failed pre-flight removes it again.
    let cancel = CancellationToken::new();
    state
        .ai_sessions
        .lock()
        .expect("ai session registry poisoned")
        .insert(id.clone(), cancel.clone());

    // `/ai/...` is read-only: clone a player-visible snapshot under a *short*
    // lock, then drop the lock before the (potentially long) network round trip
    // so `/state` and `/action` stay responsive during the analysis. The clone
    // stays in server memory; it is never serialized to the model (the payload
    // is built from the visible-only `BoardView`), so privacy is preserved.
    let game = state.game.lock().expect("game state poisoned").clone();

    match state.guide.suggest(&game, req, cancel).await {
        Ok(stream) => {
            let guard = SessionGuard::new(state.ai_sessions.clone(), id);
            Sse::new(guard_stream(stream.map(to_event), guard)).into_response()
        }
        Err(preflight) => {
            state
                .ai_sessions
                .lock()
                .expect("ai session registry poisoned")
                .remove(&id);
            preflight_response(preflight)
        }
    }
}

/// `POST /ai/guide/{id}/interrupt`: cancels the upstream generation for the
/// analysis session `{id}`. The SSE connection stays open; the interrupt event
/// is emitted on that stream (`{kind:"interrupt",reason:"user_interrupt"}`).
async fn handle_user_interrupt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let token = state
        .ai_sessions
        .lock()
        .expect("ai session registry poisoned")
        .get(&id)
        .cloned();
    match token {
        Some(token) => {
            token.cancel();
            StatusCode::NO_CONTENT.into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Removes a session's cancel token from the registry when the SSE stream ends
/// or the client disconnects, so a finished analysis doesn't leave a stale
/// entry behind. A `Drop` guard makes the cleanup robust against both a clean
/// `[DONE]` end and a mid-stream client abort.
struct SessionGuard {
    registry: Arc<Mutex<HashMap<String, CancellationToken>>>,
    id: String,
}

impl SessionGuard {
    fn new(registry: Arc<Mutex<HashMap<String, CancellationToken>>>, id: String) -> Self {
        Self { registry, id }
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.registry
            .lock()
            .expect("ai session registry poisoned")
            .remove(&self.id);
    }
}

/// Maps one `Guide::suggest` stream item to an SSE event. `Ok(Done)` becomes
/// the `[DONE]` terminator; an interrupt `Err(reason)` becomes the explicit
/// `interrupt` event (an analysis that never receives `[DONE]`).
fn to_event(item: Result<StreamChunk, InterruptReason>) -> Result<Event, axum::Error> {
    let event = match item {
        Ok(StreamChunk::ReasoningDelta(text)) => {
            Event::default().json_data(GuideEventDto::Reasoning { text })?
        }
        Ok(StreamChunk::ContentDelta(text)) => {
            Event::default().json_data(GuideEventDto::Content { text })?
        }
        Ok(StreamChunk::Done) => Event::default().data("[DONE]"),
        Err(reason) => Event::default().json_data(GuideEventDto::Interrupt { reason })?,
    };
    Ok(event)
}

/// Holds a `Drop` guard across a stream's lifetime, so the guard fires when the
/// stream is dropped (normal `[DONE]` end or a client abort).
fn guard_stream<S, G>(
    inner: S,
    guard: G,
) -> impl Stream<Item = Result<Event, axum::Error>> + Send + 'static
where
    S: Stream<Item = Result<Event, axum::Error>> + Send + 'static,
    G: Send + 'static,
{
    let mut inner = inner.boxed();
    stream::poll_fn(move |cx| {
        let _ = &guard;
        inner.as_mut().poll_next(cx)
    })
}

/// Maps a pre-flight `Guide::suggest` failure (before any content streamed)
/// into an HTTP status + a `ProviderError` body (`{kind,code,message}`); no SSE
/// is started. `AgentError::Cancelled` is a defensive branch — a cancel before
/// the stream begins surfaces as an interrupt *through* the stream, not here.
fn preflight_response(err: SuggestPreFlightError) -> Response {
    let (status, provider_error) = match err.into_inner() {
        AgentError::Provider(pe) => {
            let status = pe
                .code
                .and_then(|c| StatusCode::from_u16(c).ok())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            (status, pe)
        }
        AgentError::NoProvider => (
            StatusCode::SERVICE_UNAVAILABLE,
            ProviderError {
                kind: ProviderErrorKind::Config,
                code: None,
                message: "AI not configured: no provider selected".to_string(),
            },
        ),
        AgentError::Cancelled => (
            StatusCode::CONFLICT,
            ProviderError {
                kind: ProviderErrorKind::Config,
                code: None,
                message: "analysis cancelled before it started".to_string(),
            },
        ),
    };
    (status, Json(provider_error)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::agent::{Agent, ProviderSet};
    use crate::ai::provider::MockProvider;
    use crate::ai_adapter::{BoardFormat, Guide};
    use crate::core::{Difficulty, Features, Game, GameConfig};
    use axum::body::to_bytes;

    // The `suggest` future is held across a `tokio::sync::Mutex` guard over a
    // network await; this pins that it stays `Send` so axum accepts the route.
    fn require_send<T: Send>(_: T) {}

    #[tokio::test]
    async fn suggest_future_is_send() {
        let (state, _mock) = app_state();
        let game = state.game.lock().unwrap().clone();
        let fut = state
            .guide
            .suggest(&game, guide_request(), CancellationToken::new());
        require_send(fut);
    }

    fn app_state() -> (Arc<AppState>, MockProvider) {
        let game = Arc::new(Mutex::new(Game::with_config(GameConfig::new(
            Difficulty::Beginner,
            Features::NONE,
            None,
        ))));
        let mock = MockProvider::new();
        let mut set = ProviderSet::new();
        set.insert("mock".to_string(), Box::new(mock.clone()));
        let mut agent = Agent::new(set);
        agent.set_model("mock-model".to_string(), Some("mock"));
        let guide = Guide::new(Arc::new(tokio::sync::Mutex::new(agent)));
        let state = Arc::new(AppState {
            game,
            guide,
            ai_sessions: Arc::new(Mutex::new(HashMap::new())),
        });
        (state, mock)
    }

    fn guide_request() -> GuideRequest {
        GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        }
    }

    async fn body_as_string(resp: Response) -> String {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    // --- GuideEventDto wire shape ---

    #[test]
    fn reasoning_serializes_to_kind_and_text() {
        let value = serde_json::to_value(GuideEventDto::Reasoning {
            text: "think".into(),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"kind": "reasoning", "text": "think"})
        );
    }

    #[test]
    fn content_serializes_to_kind_and_text() {
        let value = serde_json::to_value(GuideEventDto::Content { text: "hi".into() }).unwrap();
        assert_eq!(value, serde_json::json!({"kind": "content", "text": "hi"}));
    }

    #[test]
    fn interrupt_serializes_to_kind_and_reason() {
        let value = serde_json::to_value(GuideEventDto::Interrupt {
            reason: InterruptReason::UserInterrupt,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"kind": "interrupt", "reason": "user_interrupt"})
        );
    }

    // --- pre-flight failure mapping ---

    #[tokio::test]
    async fn no_provider_maps_to_503_config() {
        let agent = Agent::new(ProviderSet::new());
        let guide = Guide::new(Arc::new(tokio::sync::Mutex::new(agent)));
        let state = Arc::new(AppState {
            game: Arc::new(Mutex::new(Game::with_config(GameConfig::new(
                Difficulty::Beginner,
                Features::NONE,
                None,
            )))),
            guide,
            ai_sessions: Arc::new(Mutex::new(HashMap::new())),
        });
        let resp = handle_guide(
            State(state.clone()),
            Path("s1".to_string()),
            Json(guide_request()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        // No SSE was started (no event-stream content type), and the session
        // token was removed so no stale entry lingers.
        assert!(state.ai_sessions.lock().unwrap().is_empty());
        let body = body_as_string(resp).await;
        assert!(body.contains("\"kind\":\"config\""));
        assert!(!body.contains("event-stream"));
    }

    // --- happy path SSE ---

    #[tokio::test]
    async fn guide_streams_reasoning_content_and_done() {
        let (state, _mock) = app_state();
        let resp = handle_guide(
            State(state.clone()),
            Path("s1".to_string()),
            Json(guide_request()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"kind\":\"reasoning\""));
        assert!(body.contains("Mock reasoning."));
        assert!(body.contains("\"kind\":\"content\""));
        assert!(body.contains("Difficulty: Beginner"));
        assert!(body.contains("data: [DONE]"));
        // A completed analysis leaves no stale session entry.
        assert!(state.ai_sessions.lock().unwrap().is_empty());
    }

    // --- interrupt cancels and emits the user_interrupt event ---

    #[tokio::test]
    async fn interrupt_cancels_and_drives_the_sse_event() {
        let (state, _mock) = app_state();
        let guide_resp = handle_guide(
            State(state.clone()),
            Path("s1".to_string()),
            Json(guide_request()),
        )
        .await;
        assert_eq!(guide_resp.status(), StatusCode::OK);

        // The interrupt route cancels the same session id.
        let ir_resp = handle_user_interrupt(State(state.clone()), Path("s1".to_string())).await;
        assert_eq!(ir_resp.status(), StatusCode::NO_CONTENT);

        // The already-open SSE emits the interrupt event instead of [DONE].
        let body = body_as_string(guide_resp).await;
        assert!(body.contains("{\"kind\":\"interrupt\",\"reason\":\"user_interrupt\"}"));
        assert!(!body.contains("[DONE]"));
        // The guard cleaned up the session entry.
        assert!(state.ai_sessions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn interrupt_unknown_session_is_not_found() {
        let (state, _mock) = app_state();
        let resp = handle_user_interrupt(State(state.clone()), Path("nope".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
