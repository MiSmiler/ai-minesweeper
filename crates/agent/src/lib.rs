//! The agent context: a provider-agnostic runtime that answers Sends
//! (ADR-0013). It is deliberately ignorant of Minesweeper — the
//! `game` crate is not a dependency, so that claim is enforced by Cargo rather
//! than by convention — and it exposes only a `Provider` seam, a `Tool`
//! abstraction, a `Session` message history, and a `run_loop`.
//!
//! The runtime splits into three layers, with one internal dependency edge
//! (`agent → provider`) and nothing depending on `protocol`'s consumer:
//!
//! - [`protocol`]: provider-agnostic value types and the wire contract
//!   (`Message`, `ChatRequest`, `StreamChunk`, `ProviderError`, ...) plus
//!   `ContentBlock`'s multi-modal internal<->wire shape.
//! - [`provider`]: the `Provider` seam and `ProviderStream`, plus the real
//!   `DeepSeek` backend (issue #116) and the offline `MockProvider` used by
//!   tests and embedders.
//! - [`agent`]: the engine — `Agent`, `Tool`, `Session`, `ProviderSet`,
//!   `run_loop` — which depends on `provider` and calls into `protocol`.
//!
//! The caller-facing names are re-exported at the crate root, so a consumer
//! writes `agent::Agent` / `agent::Message` / `agent::DeepSeek` instead of
//! naming a layer.

pub mod agent;
pub mod protocol;
pub mod provider;

pub use agent::{Agent, AgentError, ProviderSet, Session, ThinkingLevel, Tool};
pub use protocol::{
    ChatRequest, ContentBlock, Message, ProviderError, ProviderErrorKind, ReasoningEffort,
    StreamChunk, ThinkingMode, ThinkingToggle, ToolCall, ToolDecl,
};
pub use provider::{DeepSeek, DeepSeekConfig, MockProvider, Provider, ProviderStream};
