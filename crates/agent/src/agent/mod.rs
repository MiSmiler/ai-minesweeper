//! The agent engine: `Agent`, `Tool`, `Session`, `ProviderSet`, `run_loop`
//! (issue #113, ADR-0013). This is the only module that depends on
//! [`crate::provider`]; it consumes the `Provider` seam to implement
//! streaming, single-turn aggregation (`complete_once`), and the multi-turn
//! tool loop (`run_loop`).

use std::sync::{Arc, Mutex};
use std::task::Poll;

use async_trait::async_trait;
use futures::{Stream, StreamExt, stream};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    ChatRequest, Message, ProviderError, ProviderErrorKind, ReasoningEffort, StreamChunk,
    ThinkingMode, ThinkingToggle, ToolCall, ToolDecl,
};
use crate::provider::Provider;

/// The reasoning depth the agent should use for a turn (issue #122). Owned by
/// the `Agent` — the engine decides how deep to think — and translated onto the
/// provider-agnostic `ChatRequest` fields (`reasoning_effort` / `thinking`).
/// `Off` disables thinking mode; the rest set the effort. `low` is the default.
/// It is also the `SendRequest.thinking_level` wire value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Low,
    High,
    Max,
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        Self::Low
    }
}

/// Maps the agent's reasoning depth onto the provider-agnostic `ChatRequest`
/// fields. `Off` disables thinking (no effort); the rest enable it at the
/// matching effort; `None` leaves the provider default.
fn thinking_to_wire(
    level: Option<ThinkingLevel>,
) -> (Option<ReasoningEffort>, Option<ThinkingToggle>) {
    match level {
        Some(ThinkingLevel::Off) => (
            None,
            Some(ThinkingToggle {
                r#type: ThinkingMode::Disabled,
            }),
        ),
        Some(ThinkingLevel::Low) => (
            Some(ReasoningEffort::Low),
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled,
            }),
        ),
        Some(ThinkingLevel::High) => (
            Some(ReasoningEffort::High),
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled,
            }),
        ),
        Some(ThinkingLevel::Max) => (
            Some(ReasoningEffort::Max),
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled,
            }),
        ),
        None => (None, None),
    }
}

/// A tool the `Agent` can call on the model's behalf (a Minesweeper action in
/// the adapter layer). `decl` is what the model sees; `call` executes it.
#[async_trait]
pub trait Tool: Send + Sync {
    fn decl(&self) -> ToolDecl;
    async fn call(&self, args: serde_json::Value) -> Result<String, String>;
}

/// The Session a turn runs against, as a cheap cloneable handle: the
/// messages live in one shared `Vec`, so every clone sees the same history.
/// A turn lands through [`Session::commit`], which the agent calls only when a
/// stream reaches `Done` — an interrupted, failed, or unread turn never
/// enters here.
#[derive(Clone, Default)]
pub struct Session {
    messages: Arc<Mutex<Vec<Message>>>,
}

impl Session {
    /// An empty session; the first committed turn carries its own `System`.
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot of the committed messages.
    pub fn messages(&self) -> Vec<Message> {
        self.messages.lock().expect("session poisoned").clone()
    }

    /// Whether any turn has been committed yet.
    pub fn is_empty(&self) -> bool {
        self.messages.lock().expect("session poisoned").is_empty()
    }

    /// Appends a committed turn under one lock, so a turn lands atomically.
    pub fn commit(&self, messages: Vec<Message>) {
        self.messages
            .lock()
            .expect("session poisoned")
            .extend(messages);
    }
}

/// Why a Send produced no Turn. The variant set is position-restricted:
/// [`SendError::Interrupted`] appears only as a stream item and
/// [`SendError::Provider`] appears both as the delivery-time `Err` and as a
/// stream item. The compiler cannot enforce this, so it is documented here.
#[derive(Debug, PartialEq)]
pub enum SendError {
    /// The caller interrupted the Send; it commits no Turn. It is an act, not
    /// a cause — a Provider failing mid-stream is a `Provider` failure.
    Interrupted,
    /// The Provider/model exchange failed. The cause travels intact, in the
    /// same variant at delivery and mid-stream.
    Provider(ProviderError),
}

/// The `Config` failure of an Agent with no provider selected: formed at the
/// source rather than translated back into a `ProviderError` by a consumer.
fn no_provider_error() -> ProviderError {
    ProviderError {
        kind: ProviderErrorKind::Config,
        code: None,
        message: "AI not configured: no provider selected".to_string(),
    }
}

/// A named registry of providers, kept in insertion order (issue #113).
/// `Box<dyn Provider>` keeps the set heterogeneous; the `Agent` looks
/// providers up by name. The order matters: `names()` returns them in the
/// order they were inserted.
#[derive(Default)]
pub struct ProviderSet {
    providers: Vec<(String, Box<dyn Provider>)>,
}

impl ProviderSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a provider under `name`. Re-inserting a known name replaces
    /// it in place, preserving insertion order.
    pub fn insert(&mut self, name: impl Into<String>, provider: Box<dyn Provider>) {
        let name = name.into();
        if let Some(entry) = self.providers.iter_mut().find(|(n, _)| n == &name) {
            entry.1 = provider;
        } else {
            self.providers.push((name, provider));
        }
    }

    /// Looks up a provider by name.
    pub fn get(&self, name: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.as_ref())
    }

    /// The registered provider names, in insertion order.
    // `names` is a public seam consumed by a test and by the adapter layer;
    // the binary doesn't read it yet.
    #[allow(dead_code)]
    pub fn names(&self) -> Vec<&str> {
        self.providers.iter().map(|(n, _)| n.as_str()).collect()
    }
}

/// The runtime engine: holds a [`ProviderSet`], the current model/provider,
/// and the registered [`Tool`]s.
pub struct Agent {
    providers: ProviderSet,
    current_provider: String,
    current_model: String,
    tools: Vec<Arc<dyn Tool>>,
    /// The reasoning depth for turns; `None` leaves the provider default.
    /// Owned as the agent's semantic `ThinkingLevel` and translated onto the
    /// `ChatRequest` fields in [`Agent::stream`] (issue #122).
    thinking_level: Option<ThinkingLevel>,
}

impl Agent {
    /// Builds an `Agent` from a [`ProviderSet`]. The model/provider must be
    /// chosen with [`Agent::set_model`] before a turn can run.
    pub fn new(providers: ProviderSet) -> Self {
        Self {
            providers,
            current_provider: String::new(),
            current_model: String::new(),
            tools: Vec::new(),
            thinking_level: None,
        }
    }

    /// Sets the current model, optionally switching the provider. A `None`
    /// provider keeps whatever is currently selected.
    pub fn set_model(&mut self, model: String, provider: Option<&str>) {
        self.current_model = model;
        if let Some(provider) = provider {
            self.current_provider = provider.to_string();
        }
    }

    /// Sets the reasoning depth for subsequent [`Agent::stream`] calls; `None`
    /// leaves the provider default. Mirrors [`Agent::set_model`]: the `AiPlayer`
    /// locks the agent and sets it once per request.
    pub fn set_thinking_level(&mut self, level: Option<ThinkingLevel>) {
        self.thinking_level = level;
    }

    /// The currently selected provider name.
    #[allow(dead_code)]
    pub fn current_provider(&self) -> &str {
        &self.current_provider
    }

    /// The currently selected model name (fills `ChatRequest.model`).
    #[allow(dead_code)]
    pub fn current_model(&self) -> &str {
        &self.current_model
    }

    /// Registers a [`Tool`] for `run_loop` to call. Consumed by the adapter
    /// (issue #115); the runtime itself only calls tools it was given.
    #[allow(dead_code)]
    pub fn add_tool(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Loads the current provider/model without starting a Turn: a missing or
    /// unknown provider is a `Config` [`ProviderError`], as is a provider-side
    /// config failure; a transport failure is an `Upstream` one.
    /// `AiPlayer::create_session` calls this so an unconfigured AI fails before
    /// the first Send.
    pub async fn load(&self) -> Result<(), ProviderError> {
        let provider = self
            .providers
            .get(&self.current_provider)
            .ok_or_else(no_provider_error)?;
        provider.load(&self.current_model).await
    }

    /// Starts a chat stream for one Reply against the current provider: the
    /// model sees the session's committed messages plus `pending` (the first
    /// Reply carries its `System`). The stream maps every `ProviderError` to
    /// [`SendError::Provider`] (the cause intact) and reports a fired `cancel`
    /// as [`SendError::Interrupted`]. No provider is a `Config`
    /// [`SendError::Provider`] formed here.
    ///
    /// The Reply lands in `session` only when the stream reaches `Done`, as
    /// `pending` + the assistant message, under one lock. An
    /// interrupt, an error, or dropping the stream unread commits nothing.
    pub async fn stream(
        &self,
        session: &Session,
        pending: Vec<Message>,
        cancel: CancellationToken,
    ) -> Result<impl Stream<Item = Result<StreamChunk, SendError>> + Send + use<>, SendError> {
        let provider = self
            .providers
            .get(&self.current_provider)
            .ok_or_else(|| SendError::Provider(no_provider_error()))?;
        let (reasoning_effort, thinking) = thinking_to_wire(self.thinking_level);
        let mut messages = session.messages();
        messages.extend(pending.iter().cloned());
        let req = ChatRequest {
            messages,
            model: self.current_model.clone(),
            stream: true,
            tools: self.tools.iter().map(|t| t.decl()).collect(),
            reasoning_effort,
            thinking,
        };
        let inner = provider
            .stream_chat(req, cancel.clone())
            .await
            .map_err(SendError::Provider)?;
        let canceller = cancel.clone();
        let mut inner = inner;
        let committed = session.clone();
        let mut pending = Some(pending);
        let mut content = String::new();
        let mut reasoning: Option<String> = None;
        let mut reported_cancel = false;
        Ok(stream::poll_fn(move |cx| {
            if canceller.is_cancelled() {
                if reported_cancel {
                    return Poll::Ready(None);
                }
                reported_cancel = true;
                return Poll::Ready(Some(Err(SendError::Interrupted)));
            }
            match Stream::poll_next(inner.as_mut(), cx) {
                Poll::Ready(Some(Ok(StreamChunk::Done))) => {
                    if let Some(mut turn) = pending.take() {
                        turn.push(Message::Assistant {
                            content: std::mem::take(&mut content),
                            reasoning_content: reasoning.take(),
                            tool_calls: None,
                        });
                        committed.commit(turn);
                    }
                    Poll::Ready(Some(Ok(StreamChunk::Done)))
                }
                Poll::Ready(Some(Ok(StreamChunk::ReasoningDelta(text)))) => {
                    reasoning.get_or_insert_with(String::new).push_str(&text);
                    Poll::Ready(Some(Ok(StreamChunk::ReasoningDelta(text))))
                }
                Poll::Ready(Some(Ok(StreamChunk::ContentDelta(text)))) => {
                    content.push_str(&text);
                    Poll::Ready(Some(Ok(StreamChunk::ContentDelta(text))))
                }
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(SendError::Provider(e)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }))
    }

    /// Runs a single streaming Reply and aggregates it into one
    /// [`Message::Assistant`]. The Reply lands in `session` (see
    /// [`Agent::stream`]); it is returned too, for inspection.
    pub async fn complete_once(
        &self,
        session: &Session,
        pending: Vec<Message>,
        cancel: CancellationToken,
    ) -> Result<Message, SendError> {
        let mut stream = self.stream(session, pending, cancel).await?;
        let mut content = String::new();
        let mut reasoning = None;
        while let Some(chunk) = stream.next().await {
            match chunk? {
                StreamChunk::ReasoningDelta(text) => {
                    reasoning.get_or_insert_with(String::new).push_str(&text);
                }
                StreamChunk::ContentDelta(text) => content.push_str(&text),
                StreamChunk::Done => break,
            }
        }
        Ok(Message::Assistant {
            content,
            reasoning_content: reasoning,
            tool_calls: None,
        })
    }

    /// The multi-Reply tool loop: stream one Reply; if it requests tool calls,
    /// execute them and continue with the results as `pending`, until a Reply
    /// has no calls. Each Reply lands in `session` via [`Agent::stream`].
    /// A skeleton in this ticket; exercised by the adapter (issue #115).
    #[allow(dead_code)]
    pub async fn run_loop(
        &self,
        session: &Session,
        pending: Vec<Message>,
        cancel: CancellationToken,
    ) -> Result<Message, SendError> {
        let mut pending = pending;
        loop {
            let reply = self.complete_once(session, pending, cancel.clone()).await?;
            let tool_calls = match &reply {
                Message::Assistant {
                    tool_calls: Some(calls),
                    ..
                } if !calls.is_empty() => calls.clone(),
                _ => return Ok(reply),
            };
            let mut next = Vec::with_capacity(tool_calls.len());
            for call in tool_calls {
                let content = match self.call_tool(&call).await {
                    Ok(content) => content,
                    // A failed or unknown tool is fed back as a Tool message
                    // so the model can recover; the loop then continues
                    // (an AiPlay concern).
                    Err(err) => err,
                };
                next.push(Message::Tool {
                    tool_call_id: call.id,
                    content,
                });
            }
            pending = next;
        }
    }

    /// Resolves and invokes a single [`ToolCall`]. The `Err(String)` is a
    /// human-readable reason (unknown tool, or the tool's own failure).
    #[allow(dead_code)]
    async fn call_tool(&self, call: &ToolCall) -> Result<String, String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.decl().name == call.name)
            .ok_or_else(|| format!("unknown tool '{}'", call.name))?;
        tool.call(call.arguments.clone()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ContentBlock;
    use crate::provider::{MockProvider, Provider, ProviderStream};

    fn agent_with_mock(model: &str, provider_name: &str) -> (Agent, MockProvider) {
        let mock = MockProvider::new();
        let mut set = ProviderSet::new();
        set.insert(provider_name.to_string(), Box::new(mock.clone()));
        let mut agent = Agent::new(set);
        agent.set_model(model.to_string(), Some(provider_name));
        (agent, mock)
    }

    fn system_message(text: &str) -> Message {
        Message::System {
            content: text.into(),
        }
    }

    fn user_message(text: &str) -> Message {
        Message::User {
            content: vec![ContentBlock::Text(text.into())],
        }
    }

    /// The `pending` a first turn carries: the System plus the user's text.
    fn first_turn(text: &str) -> Vec<Message> {
        vec![system_message("sys"), user_message(text)]
    }

    #[tokio::test]
    async fn complete_once_with_mock_returns_assistant_reply() {
        let (agent, mock) = agent_with_mock("mock-model", "mock");
        let session = Session::new();
        let reply = agent
            .complete_once(&session, first_turn("hello"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            reply,
            Message::Assistant {
                content: "hello".into(),
                reasoning_content: Some("Mock reasoning.".into()),
                tool_calls: None,
            }
        );

        // The request that reached the provider matches the contract: model
        // filled by `current_model`, `stream` on, roles in order.
        let req = mock.last_request().expect("mock recorded a request");
        assert_eq!(req.model, "mock-model");
        assert!(req.stream);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(
            req.messages[0],
            Message::System {
                content: "sys".into()
            }
        );
        assert_eq!(
            req.messages[1],
            Message::User {
                content: vec![ContentBlock::Text("hello".into())],
            }
        );
        // The turn landed in the session: the pending pair plus the reply.
        assert_eq!(
            session.messages(),
            vec![
                system_message("sys"),
                user_message("hello"),
                Message::Assistant {
                    content: "hello".into(),
                    reasoning_content: Some("Mock reasoning.".into()),
                    tool_calls: None,
                },
            ]
        );
    }

    #[test]
    fn session_commit_appends_messages() {
        let session = Session::new();
        assert!(session.is_empty());
        session.commit(vec![system_message("sys")]);
        assert!(!session.is_empty());
        assert_eq!(session.messages(), vec![system_message("sys")]);
        session.commit(vec![user_message("hi")]);
        assert_eq!(session.messages().len(), 2);
    }

    #[test]
    fn session_is_a_shared_handle_across_clones() {
        let session = Session::new();
        let other = session.clone();
        other.commit(vec![system_message("sys")]);
        assert_eq!(session.messages(), vec![system_message("sys")]);
    }

    #[tokio::test]
    async fn stream_commits_pending_and_reply_on_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = Session::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert_eq!(
            session.messages(),
            vec![
                system_message("sys"),
                user_message("hi"),
                Message::Assistant {
                    content: "hi".into(),
                    reasoning_content: Some("Mock reasoning.".into()),
                    tool_calls: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn stream_commits_nothing_when_dropped_before_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = Session::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        // Read the reasoning delta only, then drop the stream mid-turn.
        assert!(stream.next().await.is_some());
        drop(stream);
        assert!(session.is_empty());
    }

    #[tokio::test]
    async fn stream_commits_nothing_on_interrupt() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = Session::new();
        let cancel = CancellationToken::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), cancel.clone())
            .await
            .unwrap();
        assert!(stream.next().await.is_some());
        cancel.cancel();
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
        assert!(session.is_empty());
    }

    #[tokio::test]
    async fn stream_commits_nothing_on_a_provider_error() {
        struct ErroringProvider(ProviderError);
        #[async_trait]
        impl Provider for ErroringProvider {
            async fn stream_chat(
                &self,
                _req: ChatRequest,
                _cancel: CancellationToken,
            ) -> Result<ProviderStream, ProviderError> {
                let error = self.0.clone();
                Ok(Box::pin(stream::iter(vec![Err(error)])))
            }
        }

        let mut set = ProviderSet::new();
        set.insert(
            "mock".to_string(),
            Box::new(ErroringProvider(ProviderError {
                kind: crate::protocol::ProviderErrorKind::Upstream,
                code: Some(500),
                message: "boom".into(),
            })),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        let session = Session::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        // The mid-stream failure keeps the provider's own cause intact, not a
        // lossy bucket: kind, code and message all survive.
        match stream.next().await {
            Some(Err(SendError::Provider(pe))) => {
                assert_eq!(pe.kind, crate::protocol::ProviderErrorKind::Upstream);
                assert_eq!(pe.code, Some(500));
                assert_eq!(pe.message, "boom");
            }
            other => panic!("expected the provider error intact, got {other:?}"),
        }
        assert!(session.is_empty());
    }

    #[tokio::test]
    async fn stream_yields_reasoning_content_then_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = Session::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ReasoningDelta("Mock reasoning.".into())))
        );
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ContentDelta("hi".into())))
        );
        assert_eq!(stream.next().await, Some(Ok(StreamChunk::Done)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn cancelled_token_reports_interrupted() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = Session::new();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut stream = agent
            .stream(&session, first_turn("hi"), cancel)
            .await
            .unwrap();
        // The wrapper reports `Interrupted` once, then ends.
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn unknown_provider_is_a_config_provider_error() {
        let agent = Agent::new(ProviderSet::new());
        let session = Session::new();
        let err = agent
            .complete_once(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err, SendError::Provider(no_provider_error()));
        // A runtime failure commits nothing.
        assert!(session.is_empty());
    }

    #[test]
    fn set_model_updates_model_and_provider() {
        let mut agent = Agent::new(ProviderSet::new());
        assert_eq!(agent.current_model(), "");
        assert_eq!(agent.current_provider(), "");
        agent.set_model("deepseek".into(), Some("ds"));
        assert_eq!(agent.current_model(), "deepseek");
        assert_eq!(agent.current_provider(), "ds");
        // A `None` provider keeps the previous selection.
        agent.set_model("elsewhere".into(), None);
        assert_eq!(agent.current_model(), "elsewhere");
        assert_eq!(agent.current_provider(), "ds");
    }

    #[tokio::test]
    async fn run_loop_returns_reply_when_no_tool_calls() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = Session::new();
        let reply = agent
            .run_loop(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            reply,
            Message::Assistant {
                content: "hi".into(),
                reasoning_content: Some("Mock reasoning.".into()),
                tool_calls: None,
            }
        );
    }

    #[test]
    fn provider_set_preserves_insertion_order_and_get() {
        let mut set = ProviderSet::new();
        // `insert` takes `impl Into<String>` so a `&str` literal works.
        set.insert("b", Box::new(MockProvider::new()));
        set.insert("a", Box::new(MockProvider::new()));
        assert_eq!(set.names(), vec!["b", "a"]);
        assert!(set.get("a").is_some());
        assert!(set.get("missing").is_none());
        // Re-inserting a known name replaces it in place, keeping position.
        set.insert("b", Box::new(MockProvider::new()));
        assert_eq!(set.names(), vec!["b", "a"]);
    }

    #[tokio::test]
    async fn set_thinking_level_threads_into_the_request() {
        let (mut agent, mock) = agent_with_mock("m", "mock");
        agent.set_thinking_level(Some(ThinkingLevel::Low));
        let session = Session::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().expect("mock recorded a request");
        assert_eq!(req.reasoning_effort, Some(ReasoningEffort::Low));
        assert_eq!(
            req.thinking,
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled
            })
        );
        // The default agent carries no level, so the provider sees None.
        let (agent, _mock) = agent_with_mock("m", "mock");
        assert!(agent.thinking_level.is_none());
    }

    #[tokio::test]
    async fn off_disables_thinking_with_no_effort() {
        let (mut agent, mock) = agent_with_mock("m", "mock");
        agent.set_thinking_level(Some(ThinkingLevel::Off));
        let session = Session::new();
        let mut stream = agent
            .stream(&session, first_turn("hi"), CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().expect("mock recorded a request");
        assert_eq!(req.reasoning_effort, None);
        assert_eq!(
            req.thinking,
            Some(ThinkingToggle {
                r#type: ThinkingMode::Disabled
            })
        );
    }
}
