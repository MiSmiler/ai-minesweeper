//! SSE transport for the `/ai/...` advisor routes (issue #117, ADR-0013).
//!
//! A thin transport layer over the `ai_adapter::Guide` seam: it creates the
//! backend-owned AI Session (`POST /ai/session`), appends one board to it and
//! forwards the reply as an SSE stream terminated by `[DONE]`
//! (`POST /ai/guide/{id}`), and cancels the in-flight Send
//! (`POST /ai/guide/{id}/interrupt`).
//!
//! This module never reaches into `ai_adapter` internals and never writes to
//! the `Game` — it only takes a player-visible board snapshot (cloned under a
//! short lock) to hand to `Guide::send`. The session itself (its id, its
//! messages, its cancel token) is owned by the `Guide`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use futures::StreamExt;
use futures::stream;
use serde::Serialize;

use crate::ai::agent::AgentError;
use crate::ai::protocol::{ProviderError, ProviderErrorKind, StreamChunk};
use crate::ai_adapter::{InputMode, InterruptReason, SendError, SendRequest};

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
    Reasoning {
        text: String,
    },
    Content {
        text: String,
    },
    Interrupt {
        reason: InterruptReason,
    },
    /// The verbatim player message (the `role: user` turn), emitted first so
    /// the frontend can render the player's half of the exchange (issue #124).
    User {
        text: String,
    },
}

/// The `POST /ai/session` response: the id the frontend sends back on every
/// Send.
#[derive(Debug, Serialize)]
struct NewSessionDto {
    session_id: String,
}

/// The error body of the three session-lifecycle failures. The status code is
/// the machine signal; this is the human-readable reason.
#[derive(Debug, Serialize)]
struct ErrorDto {
    error: String,
}

/// Assembles the `/ai/...` routes onto a `Router`, given the `AppState`.
/// `server::routes` merges this into the game API router.
pub(crate) fn ai_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ai/session", post(handle_new_session))
        .route("/ai/guide/{id}", post(handle_guide))
        .route("/ai/guide/{id}/interrupt", post(handle_user_interrupt))
        .with_state(state)
}

/// `POST /ai/session`: loads the AI runtime, then replaces the live AI Session
/// with an empty one and returns its id (the old session's Send is cancelled).
/// The UI holds the discard confirm; the backend replaces unconditionally. A
/// load failure (no provider / bad key / unreachable model) maps to the same
/// pre-flight `ProviderError` body as a Send, so the frontend alerts it before
/// any Send.
async fn handle_new_session(State(state): State<Arc<AppState>>) -> Response {
    match state.guide.create_session().await {
        Ok(session_id) => Json(NewSessionDto { session_id }).into_response(),
        Err(err) => preflight_response(err),
    }
}

/// `POST /ai/guide/{id}`: appends the current board to the AI Session `{id}`
/// and downstreams the reply as SSE.
async fn handle_guide(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SendRequest>,
) -> Response {
    // An image Send must carry the screenshot it describes; reject it before
    // the session is touched, so the Send never starts.
    if req.input_mode == InputMode::Image && req.image_data_url.is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "image mode requires image_data_url".to_string(),
        );
    }

    // `/ai/...` is read-only: clone a player-visible snapshot under a *short*
    // lock, then drop the lock before the (potentially long) network round trip
    // so `/state` and `/action` stay responsive during the analysis. The clone
    // stays in server memory; it is never serialized to the model (the payload
    // is built from the visible-only `BoardView`), so privacy is preserved.
    let game = state.game.lock().expect("game state poisoned").clone();

    match state.guide.send(&id, &game, req).await {
        Ok((user_text, stream)) => {
            // Emit the player's message first, then the agent's stream (issue
            // #124). `once` and `map(to_event)` share the same item type
            // (`Result<Event, axum::Error>`) so `.chain` composes them into one
            // SSE stream; `position: fixed` is the frontend's concern.
            let user_event = Event::default().json_data(GuideEventDto::User { text: user_text });
            let sse = stream::once(async move { user_event }).chain(stream.map(to_event));
            Sse::new(sse).into_response()
        }
        Err(err) => send_error_response(err),
    }
}

/// `POST /ai/guide/{id}/interrupt`: cancels the in-flight Send of `{id}`. The
/// SSE connection stays open; the interrupt event is emitted on that stream
/// (`{kind:"interrupt",reason:"user_interrupt"}`).
async fn handle_user_interrupt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    if state.guide.interrupt(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// Maps one `Guide::send` stream item to an SSE event. `Ok(Done)` becomes the
/// `[DONE]` terminator; an interrupt `Err(reason)` becomes the explicit
/// `interrupt` event (a reply that never receives `[DONE]`).
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

/// Maps a pre-flight [`SendError`] to a status + body. The three
/// session-lifecycle failures carry `{"error": "..."}` — the status code is the
/// machine signal. Only a `PreFlight` keeps the #97/#123 `ProviderError` shape,
/// because only it is a provider failure.
fn send_error_response(err: SendError) -> Response {
    match err {
        SendError::UnknownSession => {
            error_response(StatusCode::NOT_FOUND, "unknown AI session".to_string())
        }
        SendError::Busy => error_response(
            StatusCode::CONFLICT,
            "a send is already in flight for this AI session".to_string(),
        ),
        SendError::ModeMismatch { bound, requested } => error_response(
            StatusCode::BAD_REQUEST,
            format!("input mode is locked to {bound:?}, requested {requested:?}"),
        ),
        SendError::PreFlight(err) => preflight_response(err),
    }
}

/// The `{"error": "..."}` body of the session-lifecycle failures.
fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(ErrorDto { error: message })).into_response()
}

/// Maps a pre-flight agent failure (before any content streamed) into an HTTP
/// status + a `ProviderError` body (`{kind,code,message}`); no SSE is started.
/// `AgentError::Cancelled` is a defensive branch — a cancel before the stream
/// begins surfaces as an interrupt *through* the stream, not here.
fn preflight_response(err: AgentError) -> Response {
    let (status, provider_error) = match err {
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
    use crate::ai::agent::ThinkingLevel;
    use crate::ai::agent::{Agent, ProviderSet};
    use crate::ai::provider::MockProvider;
    use crate::ai_adapter::Guide;
    use crate::core::{Difficulty, Features, Game, GameConfig};
    use axum::body::to_bytes;
    use std::sync::Mutex;

    // The `send` future is held across a `tokio::sync::Mutex` guard over a
    // network await; this pins that it stays `Send` so axum accepts the route.
    fn require_send<T: Send>(_: T) {}

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
        (Arc::new(AppState { game, guide }), mock)
    }

    fn request_in(mode: InputMode) -> SendRequest {
        SendRequest {
            input_mode: mode,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        }
    }

    fn plain_request() -> SendRequest {
        request_in(InputMode::Plain)
    }

    async fn body_as_string(resp: Response) -> String {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn send_future_is_send() {
        let (state, _mock) = app_state();
        let id = state.guide.create_session().await.unwrap();
        let game = state.game.lock().unwrap().clone();
        let fut = state.guide.send(&id, &game, plain_request());
        require_send(fut);
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

    #[test]
    fn user_serializes_to_kind_and_text() {
        let value = serde_json::to_value(GuideEventDto::User { text: "hi".into() }).unwrap();
        assert_eq!(value, serde_json::json!({"kind": "user", "text": "hi"}));
    }

    // --- POST /ai/session ---

    #[tokio::test]
    async fn new_session_returns_a_fresh_id_each_time() {
        let (state, _mock) = app_state();
        let first = new_session_id(&state).await;
        let second = new_session_id(&state).await;
        assert!(!first.is_empty());
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn new_session_without_a_provider_is_503_config() {
        let agent = Agent::new(ProviderSet::new());
        let guide = Guide::new(Arc::new(tokio::sync::Mutex::new(agent)));
        let state = Arc::new(AppState {
            game: Arc::new(Mutex::new(Game::with_config(GameConfig::new(
                Difficulty::Beginner,
                Features::NONE,
                None,
            )))),
            guide,
        });
        let resp = handle_new_session(State(state.clone())).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"kind\":\"config\""));
    }

    /// Drives `handle_new_session` and parses the `session_id` out of its body.
    async fn new_session_id(state: &Arc<AppState>) -> String {
        let resp = handle_new_session(State(state.clone())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_as_string(resp).await;
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        value["session_id"].as_str().unwrap().to_string()
    }

    // --- POST /ai/guide/{id} ---

    #[tokio::test]
    async fn guide_without_a_live_session_is_404() {
        let (state, _mock) = app_state();
        let resp = handle_guide(
            State(state.clone()),
            Path("nope".to_string()),
            Json(plain_request()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"error\""));
        assert!(!body.contains("event-stream"));
    }

    #[tokio::test]
    async fn guide_streams_reasoning_content_and_done() {
        let (state, _mock) = app_state();
        let id = state.guide.create_session().await.unwrap();
        let resp = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(plain_request()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"kind\":\"reasoning\""));
        assert!(body.contains("Mock reasoning."));
        assert!(body.contains("\"kind\":\"content\""));
        // The content delta is the mock's echo of the player's board — the
        // user turn is the bare board (ADR-0016), not a header-prefixed body.
        assert!(body.contains("........."));
        // The player's message is emitted first (issue #124).
        assert!(body.contains("\"kind\":\"user\""));
        assert!(body.contains("data: [DONE]"));
        // The reply ended, so the session's Send slot is free again.
        assert!(!state.guide.interrupt(&id));
    }

    #[tokio::test]
    async fn a_second_send_while_in_flight_is_409() {
        let (state, _mock) = app_state();
        let id = state.guide.create_session().await.unwrap();
        let first = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(plain_request()),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        // `first`'s body is still unread, so its Send is still in flight.
        let second = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(plain_request()),
        )
        .await;
        assert_eq!(second.status(), StatusCode::CONFLICT);
        drop(first);
    }

    #[tokio::test]
    async fn a_mode_switch_after_the_first_commit_is_400() {
        let (state, _mock) = app_state();
        let id = state.guide.create_session().await.unwrap();
        let first = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(plain_request()),
        )
        .await;
        // Draining the body commits the Turn and locks the InputMode.
        let _ = body_as_string(first).await;
        let second = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(request_in(InputMode::Emoji)),
        )
        .await;
        assert_eq!(second.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn image_mode_without_a_data_url_is_400() {
        let (state, _mock) = app_state();
        let id = state.guide.create_session().await.unwrap();
        let resp = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(request_in(InputMode::Image)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // The Send never started, so there is nothing to interrupt.
        assert!(!state.guide.interrupt(&id));
    }

    // --- POST /ai/guide/{id}/interrupt ---

    #[tokio::test]
    async fn interrupt_cancels_and_drives_the_sse_event() {
        let (state, _mock) = app_state();
        let id = state.guide.create_session().await.unwrap();
        let guide_resp = handle_guide(
            State(state.clone()),
            Path(id.clone()),
            Json(plain_request()),
        )
        .await;
        assert_eq!(guide_resp.status(), StatusCode::OK);

        // The interrupt route cancels the same session's in-flight Send.
        let ir_resp = handle_user_interrupt(State(state.clone()), Path(id.clone())).await;
        assert_eq!(ir_resp.status(), StatusCode::NO_CONTENT);

        // The already-open SSE emits the interrupt event instead of [DONE].
        let body = body_as_string(guide_resp).await;
        assert!(body.contains("{\"kind\":\"interrupt\",\"reason\":\"user_interrupt\"}"));
        assert!(!body.contains("[DONE]"));
        assert!(!state.guide.interrupt(&id));
    }

    #[tokio::test]
    async fn interrupt_without_an_in_flight_send_is_404() {
        let (state, _mock) = app_state();
        let resp = handle_user_interrupt(State(state.clone()), Path("nope".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
