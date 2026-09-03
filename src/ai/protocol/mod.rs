//! Provider-agnostic value types for the AI runtime (issue #113).
//!
//! `ai::protocol` defines the shared wire contract exchanged with any provider
//! — [`ChatRequest`] / [`Message`] / [`ContentBlock`] / [`ToolCall`] /
//! [`ToolDecl`] — plus the stream cell ([`StreamChunk`]) and error type
//! ([`ProviderError`]) flowing out of a provider. It knows nothing about a
//! concrete vendor or about Minesweeper; it depends on `serde` only.
//!
//! These types are consumed on the **output side**: `provider` / `ai_adapter`
//! / `server` all produce them and serialize them out (to the provider's wire
//! or to the frontend). They are never parsed back from a wire body in this
//! layer, so they carry only `Serialize` (the real DeepSeek provider parses
//! its own OpenAI wire and *constructs* these values; it does not deserialize
//! `protocol` types directly).
//!
//! Key shapes:
//! - [`Message`] is tagged externally by `role` (`system` / `user` /
//!   `assistant` / `tool`), so it serializes to the OpenAI-compatible wire.
//! - [`ContentBlock`]'s *internal* representation differs from its *wire*
//!   shape (`{type:"text",text}` / `{type:"image_url",image_url:{url}}`); the
//!   conversion is handled by hand-written serde.
//! - [`ProviderError`] serializes as `{kind,code,message}`.

use serde::Serialize;

/// A message in a conversation, tagged on the wire by its `role`.
///
/// `Message` is the single role-carrying unit the runtime and any provider
/// agree on. It serializes with `serde(tag = "role", rename_all =
/// "lowercase")`, so `System` -> `{"role":"system",...}` and so on.
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
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// A block of a `User` message content.
///
/// The internal representation is a closed list of variants; the wire shape
/// (`{type,text}` / `{type,image_url,image_url:{url}}`) is produced by
/// hand-written serde so the runtime never needs to know the OpenAI-specific
/// nesting (ADR-0013).
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    /// Plain text.
    Text(String),
    /// An image, as a data URL / base64 PNG. Constructed by the adapter
    /// (issue #115) and the future provider; the mock runtime never builds one.
    #[allow(dead_code)]
    ImageUrl(String),
}

/// The wire (OpenAI-compatible) shape of a single content block.
///
/// `ImageUrl` nests its URL inside an inner object (`{url}`); `Text` is a
/// flat `{type,text}`. We serialize through this shim so the runtime keeps the
/// simple internal enum while the provider sees the exact wire contract.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentWire {
    Text { text: String },
    ImageUrl { image_url: ImageUrlWire },
}

/// The inner `{url}` object of an `image_url` content block.
#[derive(Debug, Serialize)]
struct ImageUrlWire {
    url: String,
}

impl Serialize for ContentBlock {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ContentBlock::Text(text) => {
                ContentWire::Text { text: text.clone() }.serialize(serializer)
            }
            ContentBlock::ImageUrl(url) => ContentWire::ImageUrl {
                image_url: ImageUrlWire { url: url.clone() },
            }
            .serialize(serializer),
        }
    }
}

/// A model-requested function call (the opposite direction of [`ToolDecl`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// A function declaration handed to the model (the `tools` array).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDecl {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The request sent to a provider. `model` is required and is filled by the
/// [`crate::ai::agent::Agent`]'s `current_model`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub stream: bool,
    pub tools: Vec<ToolDecl>,
}

/// One cell of a streaming chat response.
///
/// `Done` marks a normal end of stream on the wire-adjacent contract; the
/// frontend's `[DONE]` terminator is a transport concern and never appears as
/// a block on the wire.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum StreamChunk {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,
}

/// The class of a provider failure, serialized lowercased.
/// The variants are produced by the future provider (issue #116); the mock
/// runtime never fails, so they're only exercised by tests today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum ProviderErrorKind {
    /// Misconfiguration: bad auth, unknown model, malformed request.
    Config,
    /// An upstream/transient failure: network, rate limit, server error.
    Upstream,
}

/// A provider failure, serialized as `{kind,code,message}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    /// The upstream HTTP status, when one was seen; `None` for transport
    /// failures (no HTTP response).
    pub code: Option<u16>,
    pub message: String,
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
    fn assistant_message_omits_empty_reasoning_and_tool_calls() {
        let msg = Message::Assistant {
            content: "ok".into(),
            reasoning_content: None,
            tool_calls: None,
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({"role": "assistant", "content": "ok"})
        );
    }

    #[test]
    fn assistant_message_keeps_optional_fields_when_present() {
        let msg = Message::Assistant {
            content: "ok".into(),
            reasoning_content: Some("think".into()),
            tool_calls: None,
        };
        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["reasoning_content"], serde_json::json!("think"));
        assert!(value.get("tool_calls").is_none());
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

    #[test]
    fn content_block_text_uses_wire_shape() {
        let block = ContentBlock::Text("hi".into());
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            serde_json::json!({"type": "text", "text": "hi"})
        );
    }

    #[test]
    fn content_block_image_nests_the_url() {
        let block = ContentBlock::ImageUrl("data:...png".into());
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            serde_json::json!({"type": "image_url", "image_url": {"url": "data:...png"}})
        );
    }

    #[test]
    fn provider_error_serializes_to_kind_code_message() {
        let err = ProviderError {
            kind: ProviderErrorKind::Config,
            code: Some(401),
            message: "unauthorized".into(),
        };
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            serde_json::json!({"kind": "config", "code": 401, "message": "unauthorized"})
        );
        let err = ProviderError {
            kind: ProviderErrorKind::Upstream,
            code: None,
            message: "timeout".into(),
        };
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            serde_json::json!({"kind": "upstream", "code": null, "message": "timeout"})
        );
    }

    #[test]
    fn chat_request_serializes_tools_array() {
        let req = ChatRequest {
            messages: vec![],
            model: "m".into(),
            stream: true,
            tools: vec![],
        };
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["tools"], serde_json::json!([]));
    }
}
