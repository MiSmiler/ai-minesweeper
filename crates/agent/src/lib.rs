//! The agent context: the runtime that answers Sends, without branching on
//! which vendor is behind the `Provider` seam (ADR-0013). It is deliberately
//! ignorant of Minesweeper — the
//! `game` crate is not a dependency, so that claim is enforced by Cargo rather
//! than by convention — and it exposes only a `Provider` seam, a `Tool`
//! abstraction, an `Agent` that owns the live Session, and a `run_loop`. The
//! `Session` itself never leaves the crate (ADR-0021).
//!
//! The runtime splits into two layers, with one internal dependency edge
//! (`agent → provider`):
//!
//! - [`provider`]: the `Provider` seam — its stream cell and failure type
//!   (`StreamChunk`, `ProviderError`), the OpenAI-compatible request shape the
//!   seam is defined in terms of ([`provider::openai_api`]) — plus the real
//!   `DeepSeek` backend (issue #116) and the offline `MockProvider` used by
//!   tests and embedders. The request shape is the wire the vendors speak, not
//!   a neutral vocabulary: a vendor that is not OpenAI-compatible translates
//!   it into its own wire inside its implementation.
//! - [`agent`]: the engine — `Agent`, `Tool`, `ProviderSet`, `run_loop`, and
//!   the caller-facing [`Message`] — which depends on `provider`. `Session`
//!   stays crate-internal (ADR-0021), and the [`Message`] it logs is the
//!   agent's own, of which a request's message is a lossy projection
//!   (ADR-0026).
//!
//! The caller-facing names are re-exported at the crate root, so a consumer
//! writes `agent::Agent` / `agent::Message` / `agent::DeepSeek` instead of
//! naming a layer. `Session` is deliberately not among them: a caller
//! addresses the `Agent` and never a Session.

pub mod agent;
pub mod provider;

pub use agent::{Agent, Message, ProviderSet, RunError, RunEvent, SendError, ThinkingLevel, Tool};
pub use provider::openai_api::{
    ChatRequest, ContentBlock, ReasoningEffort, ThinkingMode, ThinkingToggle, ToolCall, ToolDecl,
};
pub use provider::{
    DeepSeek, DeepSeekConfig, MockProvider, Provider, ProviderError, ProviderErrorKind,
    ProviderStream, StreamChunk,
};
