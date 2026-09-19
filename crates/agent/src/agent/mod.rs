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

use crate::provider::Provider;
use crate::provider::openai_api::{
    self, ChatRequest, ReasoningEffort, ThinkingMode, ThinkingToggle, ToolCall, ToolDecl,
};
use crate::provider::{ProviderError, ProviderErrorKind, StreamChunk};

mod message;

pub use message::Message;

/// The reasoning depth the agent should use for one Send (issue #122), mapped
/// onto the `ChatRequest` fields (`reasoning_effort` / `thinking`). It is per
/// Send — the model is fixed at construction, the depth is not — and it is
/// also the per-call `thinking_level` wire value. `Off` disables thinking
/// mode; the rest set the effort. `low` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    #[default]
    Low,
    High,
    Max,
}

/// Maps the agent's reasoning depth onto the `ChatRequest` fields. `Off`
/// disables thinking (no effort); the rest enable it at the matching effort.
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
/// leaves the crate. It carries its own `seq`, its history, and the Run in
/// flight against it. The history opens with the system prompt the Session was
/// created with; a Send appends the caller's messages as soon as it reaches the
/// Provider, and the round trip's assistant half follows on its Run's `Done` —
/// or an Interrupt marker in its place. It is the agent's own log (ADR-0026):
/// what the Provider is sent is projected from it.
struct Session {
    seq: u64,
    messages: Vec<Message>,
    in_flight: Option<CancellationToken>,
}

/// Cancels a Session's Run in flight, if any. The Session is consumed: it is
/// either the one being replaced by [`Agent::create_session`] or the one being
/// ended by [`Agent::end_session`].
fn cancel_in_flight(session: Option<Session>) {
    if let Some(cancel) = session.and_then(|session| session.in_flight) {
        cancel.cancel();
    }
}

/// One cell of a Run's stream. The Run owns this type instead of borrowing the
/// Provider's [`StreamChunk`]: a Provider never produces an
/// [`RunEvent::Interrupted`], and the caller's own act belongs on the stream
/// the caller reads.
#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,
    /// The caller's own Interrupt: the reply never lands. It is an act, not a
    /// cause — a Provider failing mid-stream is [`RunError::Provider`].
    Interrupted,
}

/// Why a Run failed: the Provider's own cause, intact, from the mid-stream
/// exchange. The caller's own act is not here — an Interrupt is a
/// [`RunEvent`].
#[derive(Debug, PartialEq)]
pub enum RunError {
    /// The Provider/model exchange failed mid-stream, after the Run had begun.
    Provider(ProviderError),
}

/// Why a Send produced no Run. [`SendError::NoSession`] and
/// [`SendError::Busy`] refuse before the exchange, and [`SendError::Provider`]
/// is the delivery-time failure — the Run never began. A failure once the Run
/// is going is a [`RunError`], so whether a failure can appear here or there
/// is a matter of position, and the types say which.
#[derive(Debug, PartialEq)]
pub enum SendError {
    /// Refused before the exchange: there is no live Session to Send into.
    NoSession,
    /// Refused before the exchange: a Send is already in flight.
    Busy,
    /// The Provider/model exchange failed at delivery. The cause travels
    /// intact; the mid-stream failure is [`RunError::Provider`].
    Provider(ProviderError),
}

/// An aggregate that spans both positions ([`Agent::complete_once`] /
/// [`Agent::run_loop`]) folds the Run's failure back into the Send's result:
/// `Provider` is the one cause both positions carry, so the fact survives
/// intact.
impl From<RunError> for SendError {
    fn from(err: RunError) -> Self {
        match err {
            RunError::Provider(error) => Self::Provider(error),
        }
    }
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

/// Ties one Send to the Session it started against: the guard appends the
/// Send's messages and frees the in-flight slot on `Drop`, but only while the
/// live Session is still the one it captured (`seq` matches). A superseded
/// Send — whose Session was replaced by `create_session` or ended by
/// `end_session` — must touch neither.
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

    /// Appends messages under one lock, so a batch lands atomically.
    fn append(&self, messages: Vec<Message>) {
        self.with_current_session(|session| session.messages.extend(messages));
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

    /// Loads the current provider/model without starting a Send: a missing or
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

    /// Loads the runtime, then replaces the live Session with one whose history
    /// opens with `system_prompt`, cancelling the previous Send. A Load failure
    /// leaves the live Session untouched.
    pub async fn create_session(&self, system_prompt: String) -> Result<(), ProviderError> {
        self.load().await?;
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut live = self.live.lock().expect("session poisoned");
        cancel_in_flight(live.take());
        *live = Some(Session {
            seq,
            messages: vec![Message::System {
                content: system_prompt,
            }],
            in_flight: None,
        });
        Ok(())
    }

    /// Ends the live Session: cancels its in-flight Run and forgets it. A
    /// no-op when there is no Session.
    pub fn end_session(&self) {
        cancel_in_flight(self.live.lock().expect("session poisoned").take());
    }

    /// Cancels the Run in flight, if any; `false` when none is in flight (or
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

    /// The live Session's history, oldest first; `None` when no Session is
    /// live. This is the log itself, not the projection the Provider is sent
    /// (ADR-0026): it may hold entries the model never reads.
    pub fn messages(&self) -> Option<Vec<Message>> {
        self.live
            .lock()
            .expect("session poisoned")
            .as_ref()
            .map(|session| session.messages.clone())
    }

    /// One Send: appends `messages` to the live Session and returns the Run
    /// once they have landed. The assistant half is appended when the stream
    /// reaches `Done`. `NoSession` and `Busy` refuse before the exchange; a
    /// failed stream leaves the caller's messages behind with no reply, and an
    /// interrupted one leaves them with an Interrupt marker where the reply
    /// would have gone. The model is fixed at construction, while `level` is
    /// per Send. One cancel token spans the Send and its Run.
    pub async fn send(
        &self,
        messages: Vec<Message>,
        level: ThinkingLevel,
    ) -> Result<impl Stream<Item = Result<RunEvent, RunError>> + Send + use<>, SendError> {
        // Admission under one lock: a Send needs a live Session and must be
        // the only one in flight. Registering the cancel token here means a
        // concurrent `interrupt` / `create_session` / `end_session` finds it.
        let (history, seq, cancel) = {
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
        // The Session's log holds more than the Provider is shown: the request
        // carries its lossy projection, which is the log minus the Interrupt
        // markers. The blocks are shared, so that filter is the whole of the
        // mapping — a filter-map, not a `From`.
        let request_messages: Vec<openai_api::Message> = history
            .into_iter()
            .chain(messages.iter().cloned())
            .filter_map(message::to_provider_message)
            .collect();
        let req = ChatRequest {
            messages: request_messages,
            model: self.current_model.clone(),
            stream: true,
            tools: self.tools.iter().map(|t| t.decl()).collect(),
            reasoning_effort,
            thinking,
        };
        // Known corner, accepted and not handled: a failure here returns
        // before anything lands, so an `interrupt` racing this exchange is
        // reported as this `Provider` failure rather than `Interrupted`, and
        // leaves no trace in the history. That follows ADR-0024 — a Session
        // records what reached the Provider, and this Send did not — and it is
        // left as is: an interruption with no caller message behind it and no
        // reply it cut is not a fact worth recording.
        let inner = provider
            .stream_chat(req, cancel.clone())
            .await
            .map_err(SendError::Provider)?;

        // The Send reached the Provider: the caller's messages are the
        // Session's history now, whatever the reply turns out to be. From here
        // on an Interrupt cuts only the assistant half, landing its marker.
        guard.append(messages);

        // The wrapper maps every `ProviderError` to [`RunError::Provider`] (the
        // cause intact) and reports a fired `cancel` as
        // [`RunEvent::Interrupted`].
        let canceller = cancel;
        let mut inner = inner;
        let mut content = String::new();
        let mut reasoning: Option<String> = None;
        let mut reported_cancel = false;
        Ok(stream::poll_fn(move |cx| {
            if canceller.is_cancelled() {
                if reported_cancel {
                    return Poll::Ready(None);
                }
                reported_cancel = true;
                // The caller's act is a fact about the log, so it lands as a
                // marker of its own. Whatever had already streamed is dropped:
                // an interrupted reply is not a Reply.
                guard.append(vec![Message::new_interrupt_marker()]);
                return Poll::Ready(Some(Ok(RunEvent::Interrupted)));
            }
            match Stream::poll_next(inner.as_mut(), cx) {
                Poll::Ready(Some(Ok(StreamChunk::Done))) => {
                    guard.append(vec![Message::new_assistant_reply(
                        std::mem::take(&mut content),
                        reasoning.take(),
                    )]);
                    Poll::Ready(Some(Ok(RunEvent::Done)))
                }
                Poll::Ready(Some(Ok(StreamChunk::ReasoningDelta(text)))) => {
                    reasoning.get_or_insert_with(String::new).push_str(&text);
                    Poll::Ready(Some(Ok(RunEvent::ReasoningDelta(text))))
                }
                Poll::Ready(Some(Ok(StreamChunk::ContentDelta(text)))) => {
                    content.push_str(&text);
                    Poll::Ready(Some(Ok(RunEvent::ContentDelta(text))))
                }
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(RunError::Provider(e)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }))
    }

    /// Runs one [`Agent::send`] and aggregates it into one
    /// [`Message::Assistant`]. The reply lands in the live Session; it is
    /// returned too, for inspection. `None` means the Run ended without a reply
    /// — the caller interrupted it, and an Interrupt marker is what landed
    /// instead.
    pub async fn complete_once(
        &self,
        pending: Vec<Message>,
        level: ThinkingLevel,
    ) -> Result<Option<Message>, SendError> {
        let mut stream = self.send(pending, level).await?;
        let mut content = String::new();
        let mut reasoning = None;
        while let Some(event) = stream.next().await {
            match event? {
                RunEvent::ReasoningDelta(text) => {
                    reasoning.get_or_insert_with(String::new).push_str(&text);
                }
                RunEvent::ContentDelta(text) => content.push_str(&text),
                RunEvent::Done => break,
                // The caller's own act cut the reply: there is none to
                // aggregate, and the marker has already landed in the log.
                RunEvent::Interrupted => return Ok(None),
            }
        }
        Ok(Some(Message::new_assistant_reply(content, reasoning)))
    }

    /// The multi-Reply tool loop: stream one Reply; if it requests tool calls,
    /// execute them and continue with the results as its next messages, until a
    /// Reply has no calls. Each Reply lands in the live Session via
    /// [`Agent::complete_once`], and `None` comes back when an Interrupt cut
    /// the Run. A skeleton for now; exercised by the adapter (issue #115).
    #[allow(dead_code)]
    pub async fn run_loop(
        &self,
        messages: Vec<Message>,
        level: ThinkingLevel,
    ) -> Result<Option<Message>, SendError> {
        let mut messages = messages;
        loop {
            let reply = match self.complete_once(messages, level).await? {
                Some(reply) => reply,
                // The caller's own act ends the whole loop, not just this
                // Reply: there is nothing left to continue with.
                None => return Ok(None),
            };
            let tool_calls = match &reply {
                Message::Assistant {
                    tool_calls: Some(calls),
                    ..
                } if !calls.is_empty() => calls.clone(),
                _ => return Ok(Some(reply)),
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
            messages = next;
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
    use crate::provider::openai_api::ContentBlock;
    use crate::provider::{MockProvider, Provider, ProviderStream};

    fn agent_with_mock(model: &str, provider_name: &str) -> (Agent, MockProvider) {
        let mock = MockProvider::new();
        let mut set = ProviderSet::new();
        set.insert(provider_name.to_string(), Box::new(mock.clone()));
        let mut agent = Agent::new(set);
        agent.set_model(model.to_string(), Some(provider_name));
        (agent, mock)
    }

    /// The system prompt every test Session is created with.
    const SYSTEM: &str = "sys";

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

    /// One Send's messages: the player's text alone. A Session's history
    /// already opens with its system prompt.
    fn user_messages(text: &str) -> Vec<Message> {
        vec![user_message(text)]
    }

    /// The live Session's history, read the way a caller reads it.
    fn history(agent: &Agent) -> Vec<Message> {
        agent.messages().unwrap_or_default()
    }

    /// One reply as the `MockProvider` produces it: the last user text echoed.
    fn reply_message(text: &str) -> Message {
        Message::new_assistant_reply(text.into(), Some("Mock reasoning.".into()))
    }

    #[tokio::test]
    async fn complete_once_records_the_user_message_and_the_reply() {
        let (agent, mock) = agent_with_mock("mock-model", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let reply = agent
            .complete_once(user_messages("hello"), ThinkingLevel::Low)
            .await
            .unwrap()
            .expect("a reply landed");
        assert_eq!(reply, reply_message("hello"));

        // The request that reached the provider matches the contract: model
        // filled by `set_model`, `stream` on, roles in order.
        let req = mock.last_request().expect("mock recorded a request");
        assert_eq!(req.model, "mock-model");
        assert!(req.stream);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(
            req.messages[0],
            openai_api::Message::System {
                content: "sys".into()
            }
        );
        assert_eq!(
            req.messages[1],
            openai_api::Message::User {
                content: vec![ContentBlock::Text("hello".into())],
            }
        );
        // The history holds the system prompt, the user's message, the reply.
        assert_eq!(
            history(&agent),
            vec![
                system_message(SYSTEM),
                user_message("hello"),
                reply_message("hello"),
            ]
        );
    }

    #[tokio::test]
    async fn a_fresh_session_holds_only_its_system_prompt() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        assert_eq!(history(&agent), vec![system_message(SYSTEM)]);
    }

    #[tokio::test]
    async fn send_without_a_live_session_is_no_session() {
        let agent = Agent::new(ProviderSet::new());
        let Err(err) = agent.send(user_messages("hi"), ThinkingLevel::Low).await else {
            panic!("expected a refusal");
        };
        assert_eq!(err, SendError::NoSession);
    }

    #[tokio::test]
    async fn a_second_concurrent_send_is_busy() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let first = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        let Err(err) = agent.send(user_messages("hi"), ThinkingLevel::Low).await else {
            panic!("expected a refusal");
        };
        assert_eq!(err, SendError::Busy);
        drop(first);
    }

    #[tokio::test]
    async fn create_session_replaces_the_session_and_cancels_the_previous_send() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        agent.create_session(SYSTEM.into()).await.unwrap();
        // The displaced Send reports the caller's interrupt on its next poll…
        assert_eq!(stream.next().await, Some(Ok(RunEvent::Interrupted)));
        assert_eq!(stream.next().await, None);
        // …and its marker lands nowhere: a superseded Send must not touch the
        // replacement's log. That is all the replacement holds.
        assert_eq!(history(&agent), vec![system_message(SYSTEM)]);
    }

    #[tokio::test]
    async fn end_session_cancels_the_send_in_flight() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        agent.end_session();
        assert_eq!(stream.next().await, Some(Ok(RunEvent::Interrupted)));
        assert_eq!(stream.next().await, None);
        // No Session is left, so the marker has nowhere to land.
        assert!(history(&agent).is_empty());
    }

    #[tokio::test]
    async fn a_consumer_without_a_session_runs_the_full_lifecycle() {
        // The goal: create → send → stream → interrupt → end, never holding a
        // `Session`.
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        assert_eq!(history(&agent), vec![system_message(SYSTEM)]);
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(RunEvent::ReasoningDelta("Mock reasoning.".into())))
        );
        assert!(agent.interrupt());
        assert_eq!(stream.next().await, Some(Ok(RunEvent::Interrupted)));
        assert_eq!(stream.next().await, None);
        // The Interrupt cut only the assistant half: the user's message landed
        // as soon as the Send reached the Provider, and the marker landed with
        // it.
        assert_eq!(
            history(&agent),
            vec![
                system_message(SYSTEM),
                user_message("hi"),
                Message::new_interrupt_marker()
            ]
        );
        agent.end_session();
        let Err(err) = agent.send(user_messages("hi"), ThinkingLevel::Low).await else {
            panic!("expected a refusal");
        };
        assert_eq!(err, SendError::NoSession);
    }

    #[tokio::test]
    async fn interrupt_reports_interrupted_once_then_ends() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert!(agent.interrupt());
        // The wrapper reports `Interrupted` once, then ends.
        assert_eq!(stream.next().await, Some(Ok(RunEvent::Interrupted)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn stream_records_the_user_message_then_the_reply_on_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert_eq!(
            history(&agent),
            vec![
                system_message("sys"),
                user_message("hi"),
                reply_message("hi"),
            ]
        );
    }

    #[tokio::test]
    async fn a_dropped_stream_leaves_the_user_message_without_a_reply() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        // Read the reasoning delta only, then drop the stream before the reply.
        assert!(stream.next().await.is_some());
        drop(stream);
        assert_eq!(
            history(&agent),
            vec![system_message(SYSTEM), user_message("hi")]
        );
        // Dropping the stream freed the in-flight slot.
        assert!(!agent.interrupt());
    }

    #[tokio::test]
    async fn an_interrupted_stream_lands_a_marker_instead_of_a_reply() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert!(stream.next().await.is_some());
        assert!(agent.interrupt());
        assert_eq!(stream.next().await, Some(Ok(RunEvent::Interrupted)));
        assert_eq!(stream.next().await, None);
        // The reasoning delta that had already streamed is not part of the
        // marker: an interrupted reply is not a Reply.
        assert_eq!(
            history(&agent),
            vec![
                system_message(SYSTEM),
                user_message("hi"),
                Message::new_interrupt_marker()
            ]
        );
    }

    #[tokio::test]
    async fn an_interrupt_marker_stays_out_of_the_next_request() {
        let (agent, mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        {
            let mut stream = agent
                .send(user_messages("hi"), ThinkingLevel::Low)
                .await
                .unwrap();
            assert!(stream.next().await.is_some());
            assert!(agent.interrupt());
            while stream.next().await.is_some() {}
        }
        let mut stream = agent
            .send(user_messages("again"), ThinkingLevel::Low)
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        // The log holds the marker; the request is the log minus it, so two
        // caller messages in a row are what the model reads.
        let req = mock.last_request().expect("mock recorded a request");
        assert_eq!(req.messages.len(), 3);
        assert!(matches!(
            req.messages[0],
            openai_api::Message::System { .. }
        ));
        assert!(matches!(req.messages[1], openai_api::Message::User { .. }));
        assert!(matches!(req.messages[2], openai_api::Message::User { .. }));
    }

    #[tokio::test]
    async fn two_sends_land_two_exchanges_and_no_marker() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        for text in ["hi", "again"] {
            let mut stream = agent
                .send(user_messages(text), ThinkingLevel::Low)
                .await
                .unwrap();
            while stream.next().await.is_some() {}
        }
        assert_eq!(
            history(&agent),
            vec![
                system_message(SYSTEM),
                user_message("hi"),
                reply_message("hi"),
                user_message("again"),
                reply_message("again"),
            ]
        );
    }

    #[tokio::test]
    async fn a_provider_error_leaves_the_user_message_without_a_reply() {
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
                kind: crate::provider::ProviderErrorKind::Upstream,
                code: Some(500),
                message: "boom".into(),
            })),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        // The mid-stream failure keeps the provider's own cause intact, not a
        // lossy bucket: kind, code and message all survive.
        match stream.next().await {
            Some(Err(RunError::Provider(pe))) => {
                assert_eq!(pe.kind, crate::provider::ProviderErrorKind::Upstream);
                assert_eq!(pe.code, Some(500));
                assert_eq!(pe.message, "boom");
            }
            other => panic!("expected the provider error intact, got {other:?}"),
        }
        assert_eq!(
            history(&agent),
            vec![system_message(SYSTEM), user_message("hi")]
        );
    }

    #[tokio::test]
    async fn stream_yields_reasoning_content_then_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(RunEvent::ReasoningDelta("Mock reasoning.".into())))
        );
        assert_eq!(
            stream.next().await,
            Some(Ok(RunEvent::ContentDelta("hi".into())))
        );
        assert_eq!(stream.next().await, Some(Ok(RunEvent::Done)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn create_session_without_a_provider_is_a_config_error() {
        let agent = Agent::new(ProviderSet::new());
        let err = agent.create_session(SYSTEM.into()).await.unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::Config);
        assert!(history(&agent).is_empty());
    }

    #[tokio::test]
    async fn set_model_selects_the_model_the_request_carries() {
        let (mut agent, mock) = agent_with_mock("first", "mock");
        agent.set_model("second".into(), None);
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert_eq!(mock.last_request().unwrap().model, "second");
    }

    #[tokio::test]
    async fn run_loop_returns_reply_when_no_tool_calls() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        agent.create_session(SYSTEM.into()).await.unwrap();
        let reply = agent
            .run_loop(user_messages("hi"), ThinkingLevel::Low)
            .await
            .unwrap()
            .expect("a reply landed");
        assert_eq!(reply, reply_message("hi"));
    }

    /// A provider whose stream yields one reasoning delta and then stays open
    /// until its `cancel` fires — the way a real transport's body parks until
    /// the cancellation it observes ends it.
    struct HangingProvider;

    #[async_trait]
    impl Provider for HangingProvider {
        async fn stream_chat(
            &self,
            _req: ChatRequest,
            cancel: CancellationToken,
        ) -> Result<ProviderStream, ProviderError> {
            let delta = stream::once(async {
                Ok::<_, ProviderError>(StreamChunk::ReasoningDelta("Mock reasoning.".into()))
            });
            let until_cancelled = stream::once(async move {
                cancel.cancelled().await;
                Ok::<_, ProviderError>(StreamChunk::Done)
            });
            Ok(Box::pin(delta.chain(until_cancelled)))
        }
    }

    #[tokio::test]
    async fn complete_once_has_no_reply_when_the_run_is_interrupted() {
        let mut set = ProviderSet::new();
        set.insert("hanging".to_string(), Box::new(HangingProvider));
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("hanging"));
        agent.create_session(SYSTEM.into()).await.unwrap();

        // The aggregation parks on the hanging stream, so the interrupt lands
        // while the Run is in flight.
        let interrupt = async {
            while !agent.interrupt() {
                tokio::task::yield_now().await;
            }
        };
        let (aggregated, ()) = tokio::join!(
            agent.complete_once(user_messages("hi"), ThinkingLevel::Low),
            interrupt
        );

        assert_eq!(aggregated.unwrap(), None);
        assert_eq!(
            history(&agent),
            vec![
                system_message(SYSTEM),
                user_message("hi"),
                Message::new_interrupt_marker()
            ]
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
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Low)
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
        agent.create_session(SYSTEM.into()).await.unwrap();
        let mut stream = agent
            .send(user_messages("hi"), ThinkingLevel::Off)
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
