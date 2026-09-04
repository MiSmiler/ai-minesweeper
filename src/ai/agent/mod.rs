//! The agent engine: `Agent`, `Tool`, `Session`, `ProviderSet`, `run_loop`
//! (issue #113, ADR-0013). This is the only module that depends on
//! [`crate::ai::provider`]; it consumes the `Provider` seam to implement
//! streaming, single-turn aggregation (`complete_once`), and the multi-turn
//! tool loop (`run_loop`).

use std::sync::Arc;
use std::task::Poll;

use async_trait::async_trait;
use futures::{Stream, StreamExt, stream};
use tokio_util::sync::CancellationToken;

use crate::ai::protocol::{ChatRequest, Message, ProviderError, StreamChunk, ToolCall, ToolDecl};
use crate::ai::provider::Provider;

/// A tool the `Agent` can call on the model's behalf (a Minesweeper action in
/// the adapter layer). `decl` is what the model sees; `call` executes it.
#[async_trait]
pub trait Tool: Send + Sync {
    fn decl(&self) -> ToolDecl;
    async fn call(&self, args: serde_json::Value) -> Result<String, String>;
}

/// The conversation history a turn runs against. It owns a `Vec<Message>` and
/// grows as the loop pushes assistant / tool messages.
pub struct Session {
    messages: Vec<Message>,
}

impl Session {
    /// A session seeded with its (optional) `System` message.
    pub fn new(system: Message) -> Self {
        Self {
            messages: vec![system],
        }
    }

    /// Appends a message.
    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// The messages so far.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
}

/// A failure surfaced out of the agent runtime.
#[derive(Debug, PartialEq)]
pub enum AgentError {
    /// The provider reported a failure.
    Provider(ProviderError),
    /// No provider was selected, or the selected provider is unknown.
    NoProvider,
    /// The operation was cancelled.
    Cancelled,
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

    /// Starts a chat stream against the current provider. The stream maps
    /// every `ProviderError` to [`AgentError::Provider`] and reports a fired
    /// `cancel` as [`AgentError::Cancelled`].
    pub async fn stream(
        &self,
        session: &Session,
        cancel: CancellationToken,
    ) -> Result<impl Stream<Item = Result<StreamChunk, AgentError>> + Send + use<>, AgentError>
    {
        let provider = self
            .providers
            .get(&self.current_provider)
            .ok_or(AgentError::NoProvider)?;
        let req = ChatRequest {
            messages: session.messages().to_vec(),
            model: self.current_model.clone(),
            stream: true,
            tools: self.tools.iter().map(|t| t.decl()).collect(),
        };
        let inner = provider
            .stream_chat(req, cancel.clone())
            .await
            .map_err(AgentError::Provider)?;
        let canceller = cancel.clone();
        let mut inner = inner;
        let mut reported_cancel = false;
        Ok(stream::poll_fn(move |cx| {
            if canceller.is_cancelled() {
                if reported_cancel {
                    return Poll::Ready(None);
                }
                reported_cancel = true;
                return Poll::Ready(Some(Err(AgentError::Cancelled)));
            }
            match Stream::poll_next(inner.as_mut(), cx) {
                Poll::Ready(Some(Ok(chunk))) => Poll::Ready(Some(Ok(chunk))),
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(AgentError::Provider(e)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }))
    }

    /// Runs a single streaming turn and aggregates it into one
    /// [`Message::Assistant`]. It reuses [`Agent::stream`].
    pub async fn complete_once(
        &self,
        session: &Session,
        cancel: CancellationToken,
    ) -> Result<Message, AgentError> {
        let mut stream = self.stream(session, cancel).await?;
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

    /// The multi-turn tool loop: stream a turn, and if it requests tool calls,
    /// execute them, push the results, and repeat until a reply has no calls.
    /// A skeleton in this ticket; exercised by the adapter (issue #115).
    #[allow(dead_code)]
    pub async fn run_loop(
        &self,
        session: &mut Session,
        cancel: CancellationToken,
    ) -> Result<Message, AgentError> {
        loop {
            let reply = self.complete_once(session, cancel.clone()).await?;
            let tool_calls = match &reply {
                Message::Assistant {
                    tool_calls: Some(calls),
                    ..
                } if !calls.is_empty() => calls.clone(),
                _ => return Ok(reply),
            };
            session.push(reply);
            for call in tool_calls {
                let content = match self.call_tool(&call).await {
                    Ok(content) => content,
                    // A failed or unknown tool is fed back as a Tool message
                    // so the model can recover; the loop then continues
                    // (AiPlay concern, #115+).
                    Err(err) => err,
                };
                session.push(Message::Tool {
                    tool_call_id: call.id,
                    content,
                });
            }
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
    use crate::ai::protocol::ContentBlock;
    use crate::ai::provider::MockProvider;

    fn agent_with_mock(model: &str, provider_name: &str) -> (Agent, MockProvider) {
        let mock = MockProvider::new();
        let mut set = ProviderSet::new();
        set.insert(provider_name.to_string(), Box::new(mock.clone()));
        let mut agent = Agent::new(set);
        agent.set_model(model.to_string(), Some(provider_name));
        (agent, mock)
    }

    fn user_session(text: &str) -> Session {
        let mut session = Session::new(Message::System {
            content: "sys".into(),
        });
        session.push(Message::User {
            content: vec![ContentBlock::Text(text.into())],
        });
        session
    }

    #[tokio::test]
    async fn complete_once_with_mock_returns_assistant_reply() {
        let (agent, mock) = agent_with_mock("mock-model", "mock");
        let session = user_session("hello");
        let reply = agent
            .complete_once(&session, CancellationToken::new())
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
    }

    #[tokio::test]
    async fn stream_yields_reasoning_content_then_done() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = user_session("hi");
        let mut stream = agent
            .stream(&session, CancellationToken::new())
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
    async fn cancelled_token_reports_cancelled() {
        let (agent, _mock) = agent_with_mock("m", "mock");
        let session = user_session("hi");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut stream = agent.stream(&session, cancel).await.unwrap();
        // The wrapper reports `Cancelled` once, then ends.
        assert_eq!(stream.next().await, Some(Err(AgentError::Cancelled)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn unknown_provider_is_no_provider() {
        let agent = Agent::new(ProviderSet::new());
        let session = user_session("hi");
        let err = agent
            .complete_once(&session, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err, AgentError::NoProvider);
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
        let mut session = user_session("hi");
        let reply = agent
            .run_loop(&mut session, CancellationToken::new())
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
}
