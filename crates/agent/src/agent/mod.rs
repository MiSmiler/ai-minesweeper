//! The agent engine: `Agent`, `Tool`, `Session`, `ProviderSet`, `run_loop`
//! (issue #113, ADR-0013). This is the only module that depends on
//! [`crate::provider`]; it consumes the `Provider` seam to implement
//! streaming, single-turn aggregation (`complete_once`), and the multi-turn
//! tool loop (`run_loop`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
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

/// The reasoning depth the agent should use for one Send (issue #122),
/// translated onto the provider-agnostic `ChatRequest` fields
/// (`reasoning_effort` / `thinking`). It is per Send — the model is fixed at
/// construction, the depth is not — and it is also the
/// `SendRequest.thinking_level` wire value. `Off` disables thinking mode; the
/// rest set the effort. `low` is the default.
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
/// matching effort.
fn thinking_to_wire(level: ThinkingLevel) -> (Option<ReasoningEffort>, Option<ThinkingToggle>) {
    match level {
        ThinkingLevel::Off => (
            None,
            Some(ThinkingToggle {
                r#type: ThinkingMode::Disabled,
            }),
        ),
        ThinkingLevel::Low => (
            Some(ReasoningEffort::Low),
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled,
            }),
        ),
        ThinkingLevel::High => (
            Some(ReasoningEffort::High),
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled,
            }),
        ),
        ThinkingLevel::Max => (
            Some(ReasoningEffort::Max),
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled,
            }),
        ),
    }
}

/// A tool the `Agent` can call on the model's behalf (a Minesweeper action in
/// the adapter layer). `decl` is what the model sees; `call` executes it.
#[async_trait]
pub trait Tool: Send + Sync {
    fn decl(&self) -> ToolDecl;
    async fn call(&self, args: serde_json::Value) -> Result<String, String>;
}

/// The live Session the `Agent` owns (ADR-0021), crate-internal: it never
/// leaves the crate. It carries its own `seq`, the committed Turns, and
/// the Send in flight against it. A Turn's messages land in `messages` only
/// when a stream reaches `Done`; an interrupted, failed or unread stream adds
/// none.
struct Session {
    seq: u64,
    messages: Vec<Message>,
    in_flight: Option<CancellationToken>,
}

/// Cancels a Session's Send in flight, if any. The Session is consumed: it is
/// either the one being replaced by [`Agent::create_session`] or the one being
/// ended by [`Agent::end_session`].
fn cancel_in_flight(session: Option<Session>) {
    if let Some(cancel) = session.and_then(|session| session.in_flight) {
        cancel.cancel();
    }
}

/// Why a Send produced no Turn. The variant set is position-restricted:
/// [`SendError::NoSession`] and [`SendError::Busy`] appear only in `send`'s
/// return value, [`SendError::Interrupted`] only as a stream item, and
/// [`SendError::Provider`] in both. The compiler cannot enforce this, so it is
/// documented here.
#[derive(Debug, PartialEq)]
pub enum SendError {
    /// Refused before the exchange: there is no live Session to Send into.
    NoSession,
    /// Refused before the exchange: a Send is already in flight.
    Busy,
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

/// Ties one Send to the Session it started against: the guard commits the Turn
/// on `Done` and frees the in-flight slot on `Drop`, but only while the live
/// Session is still the one it captured (`seq` matches). A superseded Send —
/// whose Session was replaced by `create_session` or ended by `end_session` —
/// must touch neither.
struct SendGuard {
    live: Arc<StdMutex<Option<Session>>>,
    seq: u64,
}

impl SendGuard {
    /// Runs `f` against the live Session, but only while it is still the one
    /// this Send captured (`seq` matches).
    fn with_current_session(&self, f: impl FnOnce(&mut Session)) {
        let mut live = self.live.lock().expect("session poisoned");
        if let Some(session) = live.as_mut()
            && session.seq == self.seq
        {
            f(session);
        }
    }

    /// Appends the committed Turn under one lock, so it lands atomically.
    fn commit(&self, turn: Vec<Message>) {
        self.with_current_session(|session| session.messages.extend(turn));
    }
}

impl Drop for SendGuard {
    fn drop(&mut self) {
        self.with_current_session(|session| session.in_flight = None);
    }
}

/// The runtime engine: holds a [`ProviderSet`], the model/provider fixed at
/// construction, the registered [`Tool`]s, and the live Session. Only the live
/// Session is mutable after construction, so an `Agent` is `Sync` and a caller
/// holds an `Arc<Agent>`.
pub struct Agent {
    providers: ProviderSet,
    current_provider: String,
    current_model: String,
    tools: Vec<Arc<dyn Tool>>,
    /// At most one Session is live; `None` until [`Agent::create_session`] and
    /// again after [`Agent::end_session`]. The `Arc` lets a Send's guard own a
    /// handle to the slot without borrowing the Agent.
    live: Arc<StdMutex<Option<Session>>>,
    /// The counter behind each Session's `seq`.
    next_seq: AtomicU64,
}

impl Agent {
    /// Builds an `Agent` from a [`ProviderSet`]. The model/provider must be
    /// chosen with [`Agent::set_model`] before a Session can be created.
    pub fn new(providers: ProviderSet) -> Self {
        Self {
            providers,
            current_provider: String::new(),
            current_model: String::new(),
            tools: Vec::new(),
            live: Arc::new(StdMutex::new(None)),
            next_seq: AtomicU64::new(0),
        }
    }

    /// Sets the current model, optionally switching the provider. A `None`
    /// provider keeps whatever is currently selected. Construction-time: the
    /// model is fixed once a caller holds the `Agent` behind an `Arc`.
    pub fn set_model(&mut self, model: String, provider: Option<&str>) {
        self.current_model = model;
        if let Some(provider) = provider {
            self.current_provider = provider.to_string();
        }
    }

    /// Registers a [`Tool`] for `run_loop` to call. Consumed by the adapter
    /// (issue #115); the runtime itself only calls tools it was given.
    #[allow(dead_code)]
    pub fn add_tool(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Loads the current provider/model without starting a Turn: a missing or
    /// unknown provider is a `Config` [`ProviderError`], as is a provider-side
    /// config failure; a transport failure is an `Upstream` one. Crate-internal:
    /// [`Agent::create_session`] calls this so an unconfigured AI fails before
    /// the first Send.
    async fn load(&self) -> Result<(), ProviderError> {
        let provider = self
            .providers
            .get(&self.current_provider)
            .ok_or_else(no_provider_error)?;
        provider.load(&self.current_model).await
    }

    /// Loads the runtime, then replaces the live Session with an empty one,
    /// cancelling the previous Send. A Load failure leaves the live Session
    /// untouched.
    pub async fn create_session(&self) -> Result<(), ProviderError> {
        self.load().await?;
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut live = self.live.lock().expect("session poisoned");
        cancel_in_flight(live.take());
        *live = Some(Session {
            seq,
            messages: Vec::new(),
            in_flight: None,
        });
        Ok(())
    }

    /// Ends the live Session: cancels its in-flight Send and forgets it. A
    /// no-op when there is no Session.
    pub fn end_session(&self) {
        cancel_in_flight(self.live.lock().expect("session poisoned").take());
    }

    /// Cancels the Send in flight, if any; `false` when none is in flight (or
    /// there is no live Session). The stream frees the slot itself, so the
    /// Send is free as soon as the cancelled task notices.
    pub fn interrupt(&self) -> bool {
        let live = self.live.lock().expect("session poisoned");
        match live.as_ref().and_then(|session| session.in_flight.as_ref()) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Whether the live Session holds a committed Turn yet; `false` when there
    /// is no live Session.
    pub fn has_committed_turn(&self) -> bool {
        self.live
            .lock()
            .expect("session poisoned")
            .as_ref()
            .is_some_and(|session| !session.messages.is_empty())
    }

    /// One Send: appends `pending` to the live Session, streams the reply, and
    /// commits the Turn when the stream reaches `Done`. `NoSession` and `Busy`
    /// refuse before the exchange; an interrupted, failed or unread stream
    /// commits nothing. The model is fixed at construction, while `level` is
    /// per Send. One cancel token spans the whole Send.
    pub async fn send(
        &self,
        pending: Vec<Message>,
        level: ThinkingLevel,
    ) -> Result<impl Stream<Item = Result<StreamChunk, SendError>> + Send + use<>, SendError> {
        // Admission under one lock: a Send needs a live Session and must be
        // the only one in flight. Registering the cancel token here means a
        // concurrent `interrupt` / `create_session` / `end_session` finds it.
        let (messages, seq, cancel) = {
            let mut live = self.live.lock().expect("session poisoned");
            let session = live.as_mut().ok_or(SendError::NoSession)?;
            if session.in_flight.is_some() {
                return Err(SendError::Busy);
            }
            let cancel = CancellationToken::new();
            session.in_flight = Some(cancel.clone());
            (session.messages.clone(), session.seq, cancel)
        };

        // From here the guard owns the slot: an early return (no provider, a
        // failed exchange) drops it, which frees `in_flight`.
        let guard = SendGuard {
            live: Arc::clone(&self.live),
            seq,
        };
        let provider = self
            .providers
            .get(&self.current_provider)
            .ok_or_else(|| SendError::Provider(no_provider_error()))?;
        let (reasoning_effort, thinking) = thinking_to_wire(level);
        let mut messages = messages;
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
        // The wrapper maps every `ProviderError` to [`SendError::Provider`]
        // (the cause intact) and reports a fired `cancel` as
        // [`SendError::Interrupted`]. The Turn lands through `guard` only on
        // `Done`, as `pending` + the assistant message, under one lock.
        let canceller = cancel;
        let mut inner = inner;
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
                        guard.commit(turn);
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

    /// Runs one [`Agent::send`] and aggregates it into one
    /// [`Message::Assistant`]. The Turn lands in the live Session; the reply
    /// is returned too, for inspection.
    pub async fn complete_once(
        &self,
        pending: Vec<Message>,
        level: ThinkingLevel,
    ) -> Result<Message, SendError> {
        let mut stream = self.send(pending, level).await?;
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
    /// has no calls. Each Reply lands in the live Session via
    /// [`Agent::complete_once`]. A skeleton in this ticket; exercised by the
    /// adapter (issue #115).
    #[allow(dead_code)]
    pub async fn run_loop(
        &self,
        pending: Vec<Message>,
        level: ThinkingLevel,
    ) -> Result<Message, SendError> {
        let mut pending = pending;
        loop {
            let reply = self.complete_once(pending, level).await?;
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

    /// The committed Turns of the live Session (a crate-internal read).
    fn committed(agent: &Agent) -> Vec<Message> {
        agent
            .live
            .lock()
            .expect("session poisoned")
            .as_ref()
            .map(|session| session.messages.clone())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn complete_once_with_mock_returns_assistant_reply_and_commits_the_turn() {
        let (agent, mock) = agent_with_mock("mock-model", "mock");
        agent.create_session().await.unwrap();
        let reply = agent
            .complete_once(first_turn("hello"), ThinkingLevel::Low)
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
        // filled by `set_model`, `stream` on, roles in order.
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
        // The Turn landed in the live Session: the pending pair plus the reply.
        assert_eq!(
            committed(&agent),
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

    #[tokio::test]
    async fn has_committed_turn_is_false_fresh_and_true_after_a_turn() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        assert!(!agent.has_committed_turn());
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert!(agent.has_committed_turn());
    }

    #[tokio::test]
    async fn send_without_a_live_session_is_no_session() {
        let agent = Agent::new(ProviderSet::new());
        let Err(err) = agent.send(first_turn("hi"), ThinkingLevel::Low).await else {
            panic!("expected a refusal");
        };
        assert_eq!(err, SendError::NoSession);
    }

    #[tokio::test]
    async fn a_second_concurrent_send_is_busy() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let first = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        let Err(err) = agent.send(first_turn("hi"), ThinkingLevel::Low).await else {
            panic!("expected a refusal");
        };
        assert_eq!(err, SendError::Busy);
        drop(first);
    }

    #[tokio::test]
    async fn create_session_replaces_the_session_and_cancels_the_previous_send() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        agent.create_session().await.unwrap();
        // The displaced Send reports the caller's interrupt on its next poll…
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
        // …and the replacement carries no Turn.
        assert!(!agent.has_committed_turn());
    }

    #[tokio::test]
    async fn end_session_cancels_the_send_in_flight() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        agent.end_session();
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
        assert!(!agent.has_committed_turn());
    }

    #[tokio::test]
    async fn a_consumer_without_a_session_runs_the_full_lifecycle() {
        // The goal: create → send → stream → interrupt → end, never holding a
        // `Session`.
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        assert!(!agent.has_committed_turn());
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ReasoningDelta("Mock reasoning.".into())))
        );
        assert!(agent.interrupt());
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
        assert!(!agent.has_committed_turn());
        agent.end_session();
        let Err(err) = agent.send(first_turn("hi"), ThinkingLevel::Low).await else {
            panic!("expected a refusal");
        };
        assert_eq!(err, SendError::NoSession);
    }

    #[tokio::test]
    async fn interrupt_reports_interrupted_once_then_ends() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert!(agent.interrupt());
        // The wrapper reports `Interrupted` once, then ends.
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn stream_commits_pending_and_reply_on_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert_eq!(
            committed(&agent),
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
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        // Read the reasoning delta only, then drop the stream mid-turn.
        assert!(stream.next().await.is_some());
        drop(stream);
        assert!(!agent.has_committed_turn());
        // Dropping the stream freed the in-flight slot.
        assert!(!agent.interrupt());
    }

    #[tokio::test]
    async fn stream_commits_nothing_on_interrupt() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert!(stream.next().await.is_some());
        assert!(agent.interrupt());
        assert_eq!(stream.next().await, Some(Err(SendError::Interrupted)));
        assert_eq!(stream.next().await, None);
        assert!(!agent.has_committed_turn());
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
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
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
        assert!(!agent.has_committed_turn());
    }

    #[tokio::test]
    async fn stream_yields_reasoning_content_then_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
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
    async fn create_session_without_a_provider_is_a_config_error() {
        let agent = Agent::new(ProviderSet::new());
        let err = agent.create_session().await.unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::Config);
        assert!(!agent.has_committed_turn());
    }

    #[tokio::test]
    async fn set_model_selects_the_model_the_request_carries() {
        let (mut agent, mock) = agent_with_mock("first", "mock");
        agent.set_model("second".into(), None);
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert_eq!(mock.last_request().unwrap().model, "second");
    }

    #[tokio::test]
    async fn run_loop_returns_reply_when_no_tool_calls() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let reply = agent
            .run_loop(first_turn("hi"), ThinkingLevel::Low)
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
    async fn thinking_level_threads_into_the_request() {
        let (agent, mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Low)
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
    }

    #[tokio::test]
    async fn off_disables_thinking_with_no_effort() {
        let (agent, mock) = agent_with_mock("m", "mock");
        agent.create_session().await.unwrap();
        let mut stream = agent
            .send(first_turn("hi"), ThinkingLevel::Off)
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
