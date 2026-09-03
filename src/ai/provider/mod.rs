//! The `Provider` seam: how the runtime talks to any chat backend (ADR-0013).
//!
//! [`Provider`] is the single extension point for vendors (DeepSeek today,
//! others later). [`ProviderStream`] is the streaming cell type returned by
//! [`Provider::stream_chat`]; it is a boxed, `Send` stream so a `dyn
//! Provider` can be held boxed in a [`ProviderSet`](crate::ai::agent::ProviderSet).
//!
//! The module owns only the seam. Concrete providers live next to it:
//! [`mock`] (the self-test/unit-test backend) and, in a later ticket, a real
//! DeepSeek implementation.

pub mod mock;

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use tokio_util::sync::CancellationToken;

use crate::ai::protocol::{ChatRequest, ProviderError, StreamChunk};

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
    /// is filled by the caller (the `Agent`'s `current_model`). `cancel`
    /// lets the provider abort its upstream work when cancelled.
    async fn stream_chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError>;
}
