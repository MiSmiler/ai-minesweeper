//! SSE transport for the AiPlayer's `/ai/...` routes (issue #117, ADR-0013).
//!
//! A thin transport layer over the `ai_player::AiPlayer` seam: it begins an AI
//! Session under an InputMode (`POST /ai/begin-session`), appends one board to it and
//! forwards the reply as an SSE stream terminated by `[DONE]`
//! (`POST /ai/aux-send`), cancels the in-flight Run (`POST /ai/interrupt`) and
//! ends the live Session (`POST /ai/end-session`). The live Session's message
//! list is read back through the `Agent` the binding holds (`GET /ai/messages`):
//! the list is the Session's — the binding keeps no copy of it — so that route
//! addresses the Agent.
//!
//! This module never reaches into `ai_player` internals and never writes to
//! the `Game` — it only takes a player-visible board snapshot (cloned under a
//! short lock) to hand to `AiPlayer::aux_send`. The Session (its messages
//! and its cancel token) is owned by the `Agent` behind the `AiPlayer`; the
//! transport addresses the AiPlayer and carries no id.
//!
//! Every path mirrors the method its handler calls, kebab-cased:
//! `AiPlayer::begin_session` → `/ai/begin-session`, `AiPlayer::aux_send` →
//! `/ai/aux-send`, `AiPlayer::end_session` → `/ai/end-session`,
//! `Agent::interrupt` → `/ai/interrupt`, `Agent::messages` → `/ai/messages`. A
//! read is a `GET` of the thing read; an act keeps the act's own name.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use futures::stream;
use serde::{Deserialize, Serialize};

use agent::{
    ProviderError, ProviderErrorKind, RunError as AgentRunError, RunEvent as AgentRunEvent,
    SendError as AgentSendError,
};
use ai_player::{AuxSendError, AuxSendRequest, InputMode};

use super::AppState;
use super::wire::MessagesDto;

/// The SSE wire events (issue #117). Tagged by `kind` so the frontend's
/// `RunEvent(TS)` type is isomorphic on the wire:
/// `{kind:"reasoning",text}` / `{kind:"content",text}` /
/// `{kind:"interrupted"}` / `{kind:"provider_error",error}`.
///
/// The variants carry explicit fields (struct variants) because a newtype
/// variant couldn't merge the `kind` tag into a bare value under serde's
/// internally-tagged representation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RunEvent {
    Reasoning {
        text: String,
    },
    Content {
        text: String,
    },
    /// The caller's own Interrupt: the reply never lands.
    Interrupted,
    /// The Provider's failure, carrying its own cause intact.
    ProviderError {
        error: ProviderError,
    },
    /// The verbatim player message (the `role: user` turn), emitted first so
    /// the frontend can render the player's half of the exchange (issue #124).
    User {
        text: String,
    },
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
        .route("/ai/begin-session", post(handle_begin_session))
        .route("/ai/aux-send", post(handle_aux_send))
        .route("/ai/interrupt", post(handle_interrupt))
        .route("/ai/end-session", post(handle_end_session))
        .route("/ai/messages", get(handle_messages))
        .with_state(state)
}

/// The `POST /ai/begin-session` body: the InputMode the Session is created under. Its
/// system prompt is `mode`'s, and it stays this Session's for its whole life.
#[derive(Debug, Deserialize)]
struct BeginSessionRequest {
    input_mode: InputMode,
}

/// `POST /ai/begin-session`: loads the AI runtime, then replaces the live AI Session
/// with one created under the body's `input_mode` (the old session's Send is
/// cancelled). The UI holds the discard confirm; the backend replaces
/// unconditionally. A Load failure (no provider / bad key / unreachable model)
/// maps to the same `ProviderError` body as a Send failure, so the frontend
/// alerts it before any Send.
async fn handle_begin_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BeginSessionRequest>,
) -> Response {
    match state.ai_player.begin_session(req.input_mode).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(pe) => provider_error_response(pe),
    }
}

/// `POST /ai/aux-send`: appends the current board to the live AI Session and
/// downstreams the reply as SSE.
async fn handle_aux_send(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuxSendRequest>,
) -> Response {
    // `/ai/...` is read-only: clone a player-visible snapshot under a *short*
    // lock, then drop the lock before the (potentially long) network round trip
    // so `/state` and `/action` stay responsive during the Send. The clone
    // stays in server memory; it is never serialized to the model (the payload
    // is built from the visible-only `BoardView`), so privacy is preserved.
    let game = state.game.lock().expect("game state poisoned").clone();

    match state.ai_player.aux_send(&game, req).await {
        Ok((user_text, stream)) => {
            // Emit the player's message first, then the agent's stream (issue
            // #124). `once` and `map(to_event)` share the same item type
            // (`Result<Event, axum::Error>`) so `.chain` composes them into one
            // SSE stream; `position: fixed` is the frontend's concern.
            let user_event = Event::default().json_data(RunEvent::User { text: user_text });
            let sse = stream::once(async move { user_event }).chain(stream.map(to_event));
            Sse::new(sse).into_response()
        }
        Err(err) => aux_send_error_response(err),
    }
}

/// `POST /ai/interrupt`: cancels the in-flight Run, if any. The SSE connection
/// stays open; the `{kind:"interrupted"}` event is emitted on that stream.
/// Always 204 — a Send that is not in flight is not an error.
async fn handle_interrupt(State(state): State<Arc<AppState>>) -> Response {
    state.ai_player.agent().interrupt();
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /ai/end-session`: ends the live AI Session — its in-flight Run is
/// cancelled and the Session forgotten, so the message list no longer reads and
/// an aux send refuses. Always 204 — a Session that is not live is not an
/// error.
async fn handle_end_session(State(state): State<Arc<AppState>>) -> Response {
    state.ai_player.end_session();
    StatusCode::NO_CONTENT.into_response()
}

/// `GET /ai/messages`: the live AI Session's message list, oldest first, in the
/// [`MessagesDto`] envelope. No live Session is the same 409 the aux send refuses
/// on.
/// The list is read off the `Agent`, which owns it.
async fn handle_messages(State(state): State<Arc<AppState>>) -> Response {
    match state.ai_player.agent().messages() {
        Some(messages) => Json(MessagesDto { messages }).into_response(),
        None => error_response(StatusCode::CONFLICT, "no live AI session".to_string()),
    }
}

/// Maps one Run item to an SSE event. `Ok(Done)` becomes the `[DONE]`
/// terminator; `Interrupted` and `Err(Provider)` become the explicit
/// `interrupted` / `provider_error` events (a reply that never receives
/// `[DONE]`).
fn to_event(item: Result<AgentRunEvent, AgentRunError>) -> Result<Event, axum::Error> {
    let event = match item {
        Ok(AgentRunEvent::ReasoningDelta(text)) => {
            Event::default().json_data(RunEvent::Reasoning { text })?
        }
        Ok(AgentRunEvent::ContentDelta(text)) => {
            Event::default().json_data(RunEvent::Content { text })?
        }
        Ok(AgentRunEvent::Done) => Event::default().data("[DONE]"),
        Ok(AgentRunEvent::Interrupted) => Event::default().json_data(RunEvent::Interrupted)?,
        Err(AgentRunError::Provider(error)) => {
            Event::default().json_data(RunEvent::ProviderError { error })?
        }
    };
    Ok(event)
}

/// Maps a [`AuxSendError`] to a status + body. The AiPlayer's own refusal is a
/// missing screenshot; the agent's `NoSession` / `Busy` refusals carry
/// `{"error": "..."}` (the status code is the machine signal). A Provider
/// failure at delivery keeps the `{kind,code,message}` body.
fn aux_send_error_response(err: AuxSendError) -> Response {
    match err {
        AuxSendError::MissingScreenshot => error_response(
            StatusCode::BAD_REQUEST,
            "image mode requires image_data_url".to_string(),
        ),
        AuxSendError::Agent(AgentSendError::NoSession) => {
            error_response(StatusCode::CONFLICT, "no live AI session".to_string())
        }
        AuxSendError::Agent(AgentSendError::Busy) => error_response(
            StatusCode::CONFLICT,
            "a send is already in flight".to_string(),
        ),
        AuxSendError::Agent(AgentSendError::Provider(error)) => provider_error_response(error),
    }
}

/// The `{"error": "..."}` body of the session-lifecycle failures.
fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(ErrorDto { error: message })).into_response()
}

/// Maps a [`ProviderError`] into an HTTP status + a `{kind,code,message}` body;
/// no SSE is started. Both provider failure paths land here: a Load failure
/// (session creation) and a Send failure. The status is the
/// provider's own `code` when it parses, otherwise the `kind` decides:
/// `Config` is 503 (a setup the caller cannot fix by retrying), `Upstream` is
/// 502.
fn provider_error_response(pe: ProviderError) -> Response {
    let status = pe
        .code
        .and_then(|c| StatusCode::from_u16(c).ok())
        .unwrap_or(match pe.kind {
            ProviderErrorKind::Config => StatusCode::SERVICE_UNAVAILABLE,
            ProviderErrorKind::Upstream => StatusCode::BAD_GATEWAY,
        });
    (status, Json(pe)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::MockProvider;
    use agent::ThinkingLevel;
    use agent::{Agent, ChatRequest, Provider, ProviderSet, ProviderStream};
    use ai_player::AiPlayer;
    use axum::body::to_bytes;
    use game::{Difficulty, Features, Game, GameConfig};
    use std::sync::Mutex;

    // The `aux_send` future is held across a network await; this pins that it
    // stays `Send` so axum accepts the route.
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
        let ai_player = AiPlayer::new(Arc::new(agent));
        (Arc::new(AppState { game, ai_player }), mock)
    }

    /// A provider whose `stream_chat` yields one `ProviderError` and ends, so a
    /// mid-stream failure can be driven through the SSE route.
    struct FailingProvider(ProviderError);

    #[async_trait::async_trait]
    impl Provider for FailingProvider {
        async fn stream_chat(
            &self,
            _req: ChatRequest,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<ProviderStream, ProviderError> {
            let error = self.0.clone();
            Ok(Box::pin(futures::stream::iter(vec![Err(error)])))
        }
    }

    /// An `AppState` whose `AiPlayer` runs against the given provider.
    fn app_state_with(provider: Box<dyn Provider>) -> Arc<AppState> {
        let game = Arc::new(Mutex::new(Game::with_config(GameConfig::new(
            Difficulty::Beginner,
            Features::NONE,
            None,
        ))));
        let mut set = ProviderSet::new();
        set.insert("failing".to_string(), provider);
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("failing"));
        let ai_player = AiPlayer::new(Arc::new(agent));
        Arc::new(AppState { game, ai_player })
    }

    fn plain_request() -> AuxSendRequest {
        AuxSendRequest {
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        }
    }

    /// The `POST /ai/begin-session` body for `mode`.
    fn begin_session_request(mode: InputMode) -> Json<BeginSessionRequest> {
        Json(BeginSessionRequest { input_mode: mode })
    }

    async fn body_as_string(resp: Response) -> String {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn aux_send_future_is_send() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();
        let game = state.game.lock().unwrap().clone();
        let fut = state.ai_player.aux_send(&game, plain_request());
        require_send(fut);
    }

    // --- RunEvent wire shape ---

    #[test]
    fn reasoning_serializes_to_kind_and_text() {
        let value = serde_json::to_value(RunEvent::Reasoning {
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
        let value = serde_json::to_value(RunEvent::Content { text: "hi".into() }).unwrap();
        assert_eq!(value, serde_json::json!({"kind": "content", "text": "hi"}));
    }

    #[test]
    fn interrupted_serializes_to_kind_interrupted() {
        let value = serde_json::to_value(RunEvent::Interrupted).unwrap();
        assert_eq!(value, serde_json::json!({"kind": "interrupted"}));
    }

    #[test]
    fn provider_error_serializes_to_kind_and_error() {
        let value = serde_json::to_value(RunEvent::ProviderError {
            error: ProviderError {
                kind: ProviderErrorKind::Upstream,
                code: Some(429),
                message: "rate limited".into(),
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "provider_error",
                "error": {"kind": "upstream", "code": 429, "message": "rate limited"}
            })
        );
    }

    #[test]
    fn user_serializes_to_kind_and_text() {
        let value = serde_json::to_value(RunEvent::User { text: "hi".into() }).unwrap();
        assert_eq!(value, serde_json::json!({"kind": "user", "text": "hi"}));
    }

    // --- POST /ai/begin-session ---

    #[tokio::test]
    async fn begin_session_answers_204_with_no_session_id() {
        let (state, _mock) = app_state();
        let resp = handle_begin_session(
            State(state.clone()),
            begin_session_request(InputMode::Plain),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let body = body_as_string(resp).await;
        assert!(body.is_empty());
        assert!(!body.contains("session_id"));
    }

    #[tokio::test]
    async fn begin_session_without_a_provider_is_503_config() {
        let agent = Agent::new(ProviderSet::new());
        let ai_player = AiPlayer::new(Arc::new(agent));
        let state = Arc::new(AppState {
            game: Arc::new(Mutex::new(Game::with_config(GameConfig::new(
                Difficulty::Beginner,
                Features::NONE,
                None,
            )))),
            ai_player,
        });
        let resp = handle_begin_session(
            State(state.clone()),
            begin_session_request(InputMode::Plain),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"kind\":\"config\""));
    }

    #[test]
    fn a_config_error_without_a_code_is_503() {
        let resp = provider_error_response(ProviderError {
            kind: ProviderErrorKind::Config,
            code: None,
            message: "no provider".into(),
        });
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn an_upstream_failure_without_a_code_is_502() {
        let resp = provider_error_response(ProviderError {
            kind: ProviderErrorKind::Upstream,
            code: None,
            message: "connect failed".into(),
        });
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn a_provider_code_wins_over_the_kind() {
        let resp = provider_error_response(ProviderError {
            kind: ProviderErrorKind::Upstream,
            code: Some(429),
            message: "rate limited".into(),
        });
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // --- POST /ai/aux-send ---

    #[tokio::test]
    async fn send_without_a_live_session_is_409() {
        let (state, _mock) = app_state();
        let resp = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"error\""));
        assert!(!body.contains("event-stream"));
    }

    #[tokio::test]
    async fn send_streams_reasoning_content_and_done() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();
        let resp = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_as_string(resp).await;
        assert!(body.contains("\"kind\":\"reasoning\""));
        assert!(body.contains("Mock reasoning."));
        assert!(body.contains("\"kind\":\"content\""));
        // The content delta is the mock's echo of the player's board — the
        // board is the whole user message (ADR-0016).
        assert!(body.contains("........."));
        // The player's message is emitted first (issue #124).
        assert!(body.contains("\"kind\":\"user\""));
        assert!(body.contains("data: [DONE]"));
        // The reply ended, so the session's Send slot is free again.
        assert!(!state.ai_player.agent().interrupt());
    }

    #[tokio::test]
    async fn a_mid_stream_provider_error_streams_its_own_error() {
        let state = app_state_with(Box::new(FailingProvider(ProviderError {
            kind: ProviderErrorKind::Upstream,
            code: Some(429),
            message: "rate limited".into(),
        })));
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();
        let resp = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_as_string(resp).await;
        // The provider's own kind, code and message reach the wire intact — a
        // mid-stream 429 is not collapsed into a bare timeout.
        assert!(body.contains("\"kind\":\"provider_error\""));
        assert!(body.contains("\"code\":429"));
        assert!(body.contains("rate limited"));
        assert!(!body.contains("[DONE]"));
    }

    #[tokio::test]
    async fn a_second_send_while_in_flight_is_409() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();
        let first = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(first.status(), StatusCode::OK);
        // `first`'s body is still unread, so its Send is still in flight.
        let second = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(second.status(), StatusCode::CONFLICT);
        drop(first);
    }

    #[tokio::test]
    async fn image_mode_without_a_data_url_is_400() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Image)
            .await
            .unwrap();
        let resp = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // The Send never started, so there is nothing to interrupt.
        assert!(!state.ai_player.agent().interrupt());
    }

    // --- POST /ai/interrupt ---

    #[tokio::test]
    async fn interrupt_cancels_and_drives_the_sse_event() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();
        let send_resp = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(send_resp.status(), StatusCode::OK);

        // The interrupt route cancels the in-flight Run.
        let ir_resp = handle_interrupt(State(state.clone())).await;
        assert_eq!(ir_resp.status(), StatusCode::NO_CONTENT);

        // The already-open SSE emits the interrupt event instead of [DONE].
        let body = body_as_string(send_resp).await;
        assert!(body.contains("{\"kind\":\"interrupted\"}"));
        assert!(!body.contains("[DONE]"));
        assert!(!state.ai_player.agent().interrupt());
    }

    #[tokio::test]
    async fn interrupt_without_an_in_flight_send_is_204() {
        let (state, _mock) = app_state();
        let resp = handle_interrupt(State(state.clone())).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    // --- POST /ai/end-session ---

    #[tokio::test]
    async fn end_session_answers_204_and_the_session_is_gone() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();

        let resp = handle_end_session(State(state.clone())).await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(state.ai_player.agent().messages().is_none());
    }

    #[tokio::test]
    async fn end_session_without_a_live_session_is_204() {
        let (state, _mock) = app_state();
        let resp = handle_end_session(State(state.clone())).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn end_session_drives_the_interrupt_on_the_open_sse() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();
        let send_resp = handle_aux_send(State(state.clone()), Json(plain_request())).await;
        assert_eq!(send_resp.status(), StatusCode::OK);

        let resp = handle_end_session(State(state.clone())).await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // Ending cancels the in-flight Run itself, which is why the frontend
        // sends no separate Interrupt when it closes a Session.
        let body = body_as_string(send_resp).await;
        assert!(body.contains("{\"kind\":\"interrupted\"}"));
        assert!(!body.contains("[DONE]"));
    }

    // --- GET /ai/messages ---

    #[tokio::test]
    async fn messages_without_a_live_session_is_409() {
        let (state, _mock) = app_state();
        let resp = handle_messages(State(state.clone())).await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = body_as_string(resp).await;
        assert_eq!(body, r#"{"error":"no live AI session"}"#);
    }

    #[tokio::test]
    async fn messages_carries_the_session_list_in_an_envelope() {
        let (state, _mock) = app_state();
        state
            .ai_player
            .begin_session(InputMode::Plain)
            .await
            .unwrap();

        let resp = handle_messages(State(state.clone())).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_as_string(resp).await;
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let messages = value["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(
            messages[0]["content"].as_str().unwrap(),
            InputMode::Plain.system_prompt().as_str()
        );
    }
}
