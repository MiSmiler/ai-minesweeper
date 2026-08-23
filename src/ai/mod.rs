//! The AI harness: the DeepSeek client, the tools it exposes to the model,
//! and the agent session that drives a turn of play. A client of the game
//! state — it reads the live `Game` through `core`'s public interface and is
//! deliberately not part of `core.rs`.

pub mod client;
pub mod routes;
pub mod session;
pub mod tools;
