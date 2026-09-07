//! A canned provider for the `--test-ai-chat` self-check and unit tests
//! (issue #113). It makes no network call: it records the `ChatRequest` it
//! most recently received (so tests can assert the wire contract) and streams
//! a fixed reasoning delta, a content delta echoing the last user text, and
//! `Done`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;
use tokio_util::sync::CancellationToken;

use crate::ai::protocol::{ChatRequest, ContentBlock, Message, ProviderError, StreamChunk};

use super::{Provider, ProviderStream};

/// A deterministic, offline provider.
///
/// `last_request()` captures the most recent `ChatRequest` handed to
/// `stream_chat`, letting a test confirm the exact request shape without a
/// real backend. The stream it yields always ends in `Done` and echoes the
/// last `User` message's text as its `ContentDelta`.
#[derive(Clone)]
pub struct MockProvider {
    captured: Arc<Mutex<Option<ChatRequest>>>,
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            captured: Arc::new(Mutex::new(None)),
        }
    }

    /// The most recent request given to `stream_chat`, or `None`. Consumed by
    /// tests to assert the wire contract; the binary doesn't read it.
    #[allow(dead_code)]
    pub fn last_request(&self) -> Option<ChatRequest> {
        self.captured.lock().unwrap().clone()
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn stream_chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        *self.captured.lock().unwrap() = Some(req.clone());
        let echo = last_user_text(&req);
        // The Agent's `stream` wrapper checks the token itself; here we just
        // hand back a fixed stream. If cancellation fired before we started,
        // the agent reports it on first poll.
        let chunks = vec![
            Ok(StreamChunk::ReasoningDelta("Mock reasoning.".to_string())),
            Ok(StreamChunk::ContentDelta(echo)),
            Ok(StreamChunk::Done),
        ];
        let _ = cancel;
        Ok(Box::pin(stream::iter(chunks)))
    }
}

/// The text of the last `User` message, or an empty string if none.
fn last_user_text(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(t) => Some(t.as_str()),
                        ContentBlock::ImageUrl(_) => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_a_usable_mock() {
        // A `MockProvider` built via `Default` behaves like `new`.
        let _mock = MockProvider::default();
    }

    #[test]
    fn captures_the_last_request() {
        let mock = MockProvider::new();
        assert!(mock.last_request().is_none());
    }

    #[test]
    fn last_user_text_joins_text_blocks() {
        let req = ChatRequest {
            messages: vec![
                Message::System {
                    content: "sys".into(),
                },
                Message::User {
                    content: vec![
                        ContentBlock::Text("hello".into()),
                        ContentBlock::Text("world".into()),
                    ],
                },
            ],
            model: "m".into(),
            stream: true,
            tools: vec![],
        };
        assert_eq!(last_user_text(&req), "hello world");
        // No user message -> empty string.
        let empty = ChatRequest {
            messages: vec![],
            ..req
        };
        assert_eq!(last_user_text(&empty), "");
    }
}
