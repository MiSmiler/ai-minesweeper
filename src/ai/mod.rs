//! Generic, core-decoupled AI runtime (ADR-0013): a provider-agnostic agent
//! engine, deliberately ignorant of Minesweeper. `core` stays pure; `ai`
//! reuses nothing from it and exposes only a `Provider` seam, a `Tool`
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
//!   the `--test-ai-chat` self-check and unit tests.
//! - [`agent`]: the engine — `Agent`, `Tool`, `Session`, `ProviderSet`,
//!   `run_loop` — which depends on `provider` and calls into `protocol`.

pub mod agent;
pub mod protocol;
pub mod provider;
