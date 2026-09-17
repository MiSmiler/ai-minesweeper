//! The message a Session's history is made of: the agent's own record of what
//! happened.
//!
//! It is deliberately not the provider's message
//! ([`crate::provider::openai_api`]), which is one wire's shape. A Session
//! records facts a model has no business reading — an Interrupt is one — and
//! the provider's message is a lossy projection of this one, built when the
//! request is assembled. The content blocks are shared, so dropping the
//! Interrupt marker is the whole of that projection — [`to_provider_message`].

use serde::Serialize;

use crate::provider::openai_api::{self, ContentBlock, ToolCall};

/// One entry in a Session's history, tagged on the wire by its `role`.
///
/// The history is append-only (ADR-0024): the system prompt the Session was
/// created with, the caller's messages as each Send reaches the Provider, and
/// each reply on `Done`. An Interrupt appends a marker in place of the reply —
/// an `Assistant` carrying `interrupt: true` and nothing else — so that an
/// interrupted reply is never confused with a Reply.
///
/// A Session's log holds more than the Provider is shown (ADR-0026). This type
/// is both the log's entry and the frontend's wire shape; what the Provider
/// reads is projected from it.
///
/// `interrupt` is an absence rather than a value, so it is omitted when unset,
/// exactly as `reasoning_content` and `tool_calls` are.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: Vec<ContentBlock>,
    },
    Assistant {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
        #[serde(skip_serializing_if = "is_false")]
        interrupt: bool,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// `interrupt` is absent when unset, not present as `false`.
fn is_false(flag: &bool) -> bool {
    !*flag
}

impl Message {
    /// The Interrupt marker: what lands in place of a reply the caller cut. It
    /// carries nothing else on purpose — an interrupted reply is not a Reply.
    pub(super) fn new_interrupt_marker() -> Self {
        Self::Assistant {
            content: String::new(),
            reasoning_content: None,
            tool_calls: None,
            interrupt: true,
        }
    }

    /// A Reply, ready to land: the assistant half of a Send. Nothing but a
    /// marker carries `interrupt`, and nothing carries tool calls yet.
    pub(super) fn new_assistant_reply(content: String, reasoning_content: Option<String>) -> Self {
        Self::Assistant {
            content,
            reasoning_content,
            tool_calls: None,
            interrupt: false,
        }
    }
}

/// Projects one entry of a Session's log onto the Provider's message: the lossy
/// projection applied when a `ChatRequest` is built.
///
/// `None` for the Interrupt marker — the Provider is shown what the model needs
/// to read, and "the caller stopped here" is not part of that. Every other
/// entry passes through untouched, because the content blocks are shared.
pub(super) fn to_provider_message(message: Message) -> Option<openai_api::Message> {
    match message {
        Message::Assistant {
            interrupt: true, ..
        } => None,
        Message::System { content } => Some(openai_api::Message::System { content }),
        Message::User { content } => Some(openai_api::Message::User { content }),
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            ..
        } => Some(openai_api::Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
        }),
        Message::Tool {
            tool_call_id,
            content,
        } => Some(openai_api::Message::Tool {
            tool_call_id,
            content,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_serializes_by_role() {
        let msg = Message::System {
            content: "be helpful".into(),
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({"role": "system", "content": "be helpful"})
        );
    }

    #[test]
    fn user_message_serializes_content_as_blocks() {
        let msg = Message::User {
            content: vec![ContentBlock::Text("hi".into())],
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})
        );
    }

    #[test]
    fn assistant_message_omits_an_unset_interrupt() {
        let msg = Message::new_assistant_reply("ok".into(), None);
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({"role": "assistant", "content": "ok"})
        );
    }

    #[test]
    fn assistant_message_keeps_optional_fields_when_present() {
        let msg = Message::new_assistant_reply("ok".into(), Some("think".into()));
        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["reasoning_content"], serde_json::json!("think"));
        assert!(value.get("tool_calls").is_none());
        assert!(value.get("interrupt").is_none());
    }

    #[test]
    fn an_interrupt_marker_serializes_as_an_empty_assistant() {
        let msg = Message::new_interrupt_marker();
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({"role": "assistant", "content": "", "interrupt": true})
        );
    }

    #[test]
    fn tool_message_serializes_with_role_and_id() {
        let msg = Message::Tool {
            tool_call_id: "call_1".into(),
            content: "result".into(),
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "result"})
        );
    }
}
