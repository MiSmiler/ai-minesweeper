//! The `Provider` seam: how the runtime talks to any chat backend (ADR-0013).
//!
//! [`Provider`] is the single extension point for vendors (DeepSeek today,
//! others later). [`ProviderStream`] is the streaming cell type returned by
//! [`Provider::stream_chat`]; it is a boxed, `Send` stream so a `dyn
//! Provider` can be held boxed in a [`ProviderSet`](crate::agent::ProviderSet).
//!
//! The module owns only the seam. Concrete providers live next to it:
//! [`deepseek`] (the real OpenAI-compatible backend, issue #116) and [`mock`]
//! (the offline self-test / unit-test backend). The request shape the seam is
//! defined in terms of lives in [`openai_api`].

pub mod deepseek;
pub mod mock;
pub mod openai_api;

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use self::openai_api::ChatRequest;

pub use deepseek::{DeepSeek, DeepSeekConfig};

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
// The mock provider is only referenced by unit tests (the product uses
// DeepSeek); keep the re-export available without tripping `unused_imports`
// in a non-test `cargo build`.
#[allow(unused_imports)]
pub use mock::MockProvider;

/// A stream of [`StreamChunk`]s, each possibly failing with a
/// [`ProviderError`]. Owned and `Send`, so it can be returned across `await`
/// points and driven from any task.
pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;

/// A chat backend seam. Implementations own their transport (HTTP/SSE) and
/// every vendor-specific concern; the runtime stays generic.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Starts a streaming chat, returning the [`ProviderStream`]. `req.model`
    /// is filled by the caller (the `Agent`'s `current_model`). `cancel_token`
    /// lets the provider abort its upstream work when cancelled.
    async fn stream_chat(
        &self,
        req: ChatRequest,
        cancel_token: CancellationToken,
    ) -> Result<ProviderStream, ProviderError>;

    /// Loads the provider for `model` — the provider half of `Load`: resolves
    /// its config and ensures the model is available (a provider may fetch and
    /// cache its catalog on first use), so an unconfigured provider fails
    /// before any Send starts. The default is a no-op for providers with no
    /// external config (the mock).
    async fn load(&self, _model: &str) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            message: "connect failed".into(),
        };
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            serde_json::json!({"kind": "upstream", "code": null, "message": "connect failed"})
        );
    }
}
