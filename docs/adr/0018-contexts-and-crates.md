# Contexts and crates: game, agent, ai-player, and the app as composition root

The repo carried three languages in one glossary. The game's nouns (Board, Cell, Mine, Gesture), the runtime's nouns (Agent, Provider, Session, Turn), and the binding's nouns (InputMode, the board legend, one Session per Game) sat in one flat list, and two of them collided: `ai::agent::Session` was the accumulated messages, while **AI Session** was the backend-owned session bound to one Game. The layering that kept them apart — ADR-0003's "`core` stays a pure-logic module with no serde or server dependencies", ADR-0013's "`ai` is deliberately ignorant of Minesweeper" — was a comment and a doc, not something the compiler enforced.

We split the language into three contexts and promoted each existing seam to a crate boundary:

| Context | Rust | TypeScript | Glossary |
| --- | --- | --- | --- |
| `game` | `crates/game` | `frontend/src/game` | `crates/game/CONTEXT.md` |
| `agent` | `crates/agent` | — | `crates/agent/CONTEXT.md` |
| `ai-player` | `crates/ai-player` | `frontend/src/ai-player` | `crates/ai-player/CONTEXT.md` |
| app (root) | `src/` (binary, transport) | `frontend/src/app`, `frontend/src/infra` | `CONTEXT-MAP.md` |

A context is a language boundary, not a directory: `game` and `ai-player` span both stacks, `agent` is Rust-only, and the app lives beside the code as the root package. The app is the **composition root**, not a fourth context — it owns no language beyond PlayMode and Composition. `CONTEXT-MAP.md` at the root is the authority on where each context lives.

The dependency direction is now the compiler's business: `game` and `agent` depend on nothing above them, `ai-player` depends on both, and the app depends on all three. `crates/agent` cannot see `game`, so the runtime's Minesweeper-blindness is a Cargo fact rather than a promised convention.

The runtime's lifecycle words became the agent's, with definitions that never name the Board: **Session**, **Send**, **Prepare**, **Turn**, **Load**, **InterruptReason**. The `AI` prefix went with them — `AI Session` → **Session**, `AI Agent` → **Agent** — because the prefix existed only to tell two contexts apart, and the split removes the ambiguity it was papering over. **InputMode** and **BoardView** are ai-player's, and the Board payload sentence lives with them. The policy that is not a thing — one Session per Game, the InputMode bound by the first committed Turn, a New Game ending the Session — stays in ai-player's ADRs.

Considered options:

- **Docs only** (a context map and three glossaries, directories untouched): rejected — the map would point at code that is not there, and the two structural claims above would stay comments.
- **Per-context routes and DTOs** (each crate owning its HTTP surface and its serde types, the app only merging routers): rejected for now — the transport is a thin adapter over the domain (ADR-0003), and moving it into the contexts would put axum and SSE vocabulary into their glossaries. It is worth reopening as its own decision if a context's HTTP surface grows.
- **A pure virtual workspace** (`crates/*` only, `frontend/` moved into the app crate's directory): rejected — as the root package, the app's directory already contains `frontend/`, `cargo run` still starts the app, and Vite's root never moves.
- **Keeping the `guide` name for the ai-player's central object**: deferred to #129; this record only renames the crates and the vocabulary.

Consequences:

- `cargo test` at the workspace root runs only the root package, so the gate is `cargo test --workspace`; this is recorded in `AGENTS.md` rather than left as a trap.
- Each glossary lives beside its context's code; the two app-level terms (PlayMode, Composition) live in `CONTEXT-MAP.md`.
- ADRs moved beside their context: `crates/game/docs/adr/` (0001, 0002, 0004–0010) and `crates/ai-player/docs/adr/` (0014–0017); the system-wide ones (0003, 0011, 0012, 0013) stay in `docs/adr/`. References are by number, so relocating them breaks nothing.
- ADR-0011 is amended: the frontend's slices are named after their contexts, and `gameArea.ts` / `makeGameSnapshot` moved into the game slice. ADR-0013's decision stands; its two layers are now the crates `agent` and `ai-player`.
- `crates/game` gained a `test-support` feature for `Game::with_mines` and `Game::mines`, which a consuming crate's tests need; the game crate stays serde-free.
- The ai-player's tool-loop seam (`GameHandle`, `tools()`) has no caller yet — the future `AiPlay` — and was left dormant rather than redesigned.
- Out of scope, tracked separately: #136 (the agent owning the Session lifecycle, which will shrink the binding further), #129 (retiring `guide`), #137 (the frontend's off-glossary nouns).
