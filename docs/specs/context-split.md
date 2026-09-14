# Contexts and crates: game, agent, ai-player — spec

## Problem

`CONTEXT.md` carries three languages in one glossary. The game's nouns (Board, Cell, Mine, Gesture), the runtime's nouns (Agent, Provider, Session, Turn), and the binding's nouns (InputMode, the board legend, the advisor's session lifecycle) sit in one flat list, and two of them collide: `ai::agent::Session` is the accumulated messages, while **AI Session** is the backend-owned session bound to one Game.

The layering that keeps them apart is a comment and a doc, not something the compiler enforces: ADR-0003's "`core` stays a pure-logic module with no serde or server dependencies", ADR-0013's "`ai` is deliberately ignorant of Minesweeper".

This work makes both structural: one glossary per context, and the dependency direction as a crate boundary. It changes no behaviour — it moves, renames, and documents.

Constraints: the existing suites keep passing (`cargo test --workspace`, `npm test`); ADR references are by number (72 of them), so relocating ADR files breaks no reference; the frontend and the Rust binary keep running from the repo root.

## Functional Design

### Three contexts and a composition root

`game`, `agent`, and `ai-player` are contexts — a language boundary, not a directory. A context may span both stacks (`game`, `ai-player`), sit on one side only (`agent` is Rust-only), or live beside the code as the root package (the app).

The app is the **composition root**, not a fourth context: it owns no language beyond **PlayMode** and **Composition**.

`CONTEXT-MAP.md` at the repo root records the map, those two app-level terms, and where each context lives:

| Context | Rust | TypeScript |
| --- | --- | --- |
| `game` | `crates/game` | `frontend/src/game` |
| `agent` | `crates/agent` | — |
| `ai-player` | `crates/ai-player` | `frontend/src/ai-player` |
| app (root) | `src/` (bin + transport) | `frontend/src/app`, `frontend/src/infra` |

Each context's glossary lives beside its code: `crates/game/CONTEXT.md`, `crates/agent/CONTEXT.md`, `crates/ai-player/CONTEXT.md`. The root `CONTEXT.md` is replaced by the map plus those three.

### Crates

Every seam already exists in the code; the split promotes it to a compile-time fact.

- `crates/game` — was `src/core/` (701 lines plus its 832-line test module). Dependencies stay `rand` + `tracing`; still no serde, no HTTP, and no agent.
- `crates/agent` — was `src/ai/`. After the split it cannot see `game`, so ADR-0013's claim about the Minesweeper-blind runtime is enforced by Cargo.
- `crates/ai-player` — was `src/ai_adapter/`, the one place Minesweeper knowledge is married to the runtime. `prompts/*.md` moves in with it; `include_str!` keeps its `CARGO_MANIFEST_DIR`-relative `/prompts/...` path.
- Root package `ai-minesweeper` — `src/main.rs` + `src/server/` (the routes and the `wire.rs` DTOs) + `frontend/`. The frontend stays where it is: as the root package's directory, it is already inside the app crate.

Frontend moves: `frontend/src/ai` → `frontend/src/ai-player`; `frontend/src/app/gameArea.ts` → `frontend/src/game/gameArea.ts`; `frontend/src/infra/testUtils.ts`'s `makeGameSnapshot` → `frontend/src/game/testUtils.ts`. `gameArea.ts` takes its composition-facing options (`onNewGame`, `beforeNewGame`, `onRender`) with it — those are the app's arguments to a game-context module, not a reason for the module to live in `app/`. The compositions (`main.ts`, `mode.ts`, `singleMode.ts`, `guideMode.ts`) stay in `frontend/src/app/` (ADR-0012), and `guideMode.ts` keeps its dashboard in the same file: only one caller uses it, so extracting it would add a file without adding leverage.

### Vocabulary

The runtime's lifecycle words move to `agent`, and their definitions are generalized so the agent crate never names the Board. `AI Session` becomes **Session** and `AI Agent` becomes **Agent** — the `AI` prefix was there to distinguish two contexts, and the split removes the ambiguity it was papering over. The Board-payload sentence ("a Send carries the Board rendered into the active InputMode") belongs to `ai-player`, beside **InputMode** and **BoardView**.

`crates/agent/CONTEXT.md` carries:

```md
**Agent**: The runtime that owns the model and the Provider and answers Sends. It is blind to what the messages mean. _Avoid_: assistant, bot, AI
**Session**: The messages a caller has committed: the `user` / `assistant` Turns in order. A Session holds only committed Turns — an interrupted, failed, or unread stream adds none. _Avoid_: conversation, chat, context, transcript
**Send**: One call that appends a message to a Session and asks the Agent for a reply; the reply completes the Turn. A Send that fails before any content adds no Turn. _Avoid_: analysis, request, prompt, query
**Prepare**: A Send's pre-processing — the payload the caller supplies and the checks in front of it — before any reply content arrives. A failed Prepare adds no Turn and leaves the Session live. _Avoid_: preflight, pre-flight, preamble
**Turn**: One Send together with the reply: the committed `user` / `assistant` pair. A Turn enters a Session as a unit. _Avoid_: round, exchange, message pair
**Load**: The Agent bringing its Provider and model up: the configuration is resolved and the model validated, before any Send. A Load failure leaves any live Session untouched. _Avoid_: preflight, pre-flight, startup, initialization
**InterruptReason**: Why a stream stopped early: the caller cancelled it, or the Provider failed mid-stream (rate limit, timeout, upstream error). _Avoid_: error kind, failure reason, stop reason
```

`crates/ai-player/CONTEXT.md` carries **InputMode** and **BoardView** (the Board rendered into the agent's payload). Its policy — one Session per Game, the InputMode bound by the first committed Turn, a New Game ending the Session — is stated in its ADRs (0016, 0017) rather than as new nouns: none of it is a thing, all of it is a rule.

`crates/game/CONTEXT.md` takes the game's entries unchanged. **Dialog** is deliberately not a glossary term; `conversation` → `dialog` is a code-convention rename under #137.

### ADRs

Root (`docs/adr/`): 0003, 0011, 0012, 0013. `crates/game/docs/adr/`: 0001, 0002, 0004, 0005, 0006, 0007, 0008, 0009, 0010. `crates/ai-player/docs/adr/`: 0014, 0015, 0016, 0017.

New **ADR-0018** records this split. **ADR-0011 is reopened**: the frontend's directories and its slicing both change. ADR-0013 changes wording only (`ai/` → `crates/agent`, `ai_adapter` → `crates/ai-player`); its decision stands.

### Project docs and tooling

`AGENTS.md`: the backend is described as the root package plus `crates/*`; `cargo test` becomes `cargo test --workspace` (with a root package, a plain `cargo test` silently runs only the root package); `cargo fmt` and `cargo run` are unchanged. `docs/agents/domain.md`: the multi-context layout points at `crates/<context>/docs/adr/`.

## Code Design

### Seams

**`crates/game` → `crates/ai-player`** — sits here because rendering the player-visible Board into the model's payload is the only thing that crosses, and it must be the only thing. The game context stays serde-free, HTTP-free, and agent-blind. The tool-loop half (`GameHandle` + `tools()`) has no caller yet — the future `AiPlay` — and is left dormant rather than redesigned.

**`crates/agent` → `crates/ai-player`** — sits here because the runtime is generic: this is where Minesweeper knowledge enters, and the last point at which it is absent.

**`crates/ai-player` → app** — sits here because the app is the composition root and must not know the Provider, the model, or the agent type. The app holds a `Guide`, clones a `Game` under a short lock, and forwards requests.

**app ↔ `frontend/`** — the two existing hand-written wire mirrors, plus the frontend's slice boundaries. `frontend/src/game` and `frontend/src/ai-player` never import each other; only `app/` imports both. That holds today and the split preserves it.

### Interfaces

```rust
// crates/game — the pure-rules interface, unchanged; now a crate boundary
pub struct Game;         // with_config, new_game, reveal, toggle_flag, chord, cell_view, …
pub struct Position; pub enum Difficulty; pub enum CellState; pub enum CellContent;
pub struct CellView; pub enum GameState; pub struct Features; pub type Seed;
// Invariants: no serde, no HTTP, no agent. Domain enums expose as_str() so the
// adapters reuse the canonical names instead of re-deriving a mapping.
```

```rust
// crates/agent — one import path per type; the modules stay public for the layering
pub use agent::{Agent, AgentError, ProviderSet, Session, ThinkingLevel, Tool};
pub use protocol::{Message, ContentBlock, ToolDecl, ProviderError, ProviderErrorKind,
                   StreamChunk, ChatRequest, …};
pub use provider::{DeepSeek, DeepSeekConfig, MockProvider, Provider, ProviderStream};

pub async fn load(&self) -> Result<(), AgentError>;
pub async fn stream(&self, session: &Session, pending: Vec<Message>, cancel: CancellationToken)
    -> Result<impl Stream<Item = Result<StreamChunk, AgentError>> + Send, AgentError>;
```

- Invariants: the agent is stateless — the caller owns the Session and the pending messages; a Turn lands only through `Session::commit`, which the agent calls when a stream reaches `Done`; an interrupted, failed, or unread stream commits nothing.
- Ordering: `load()` before the first Send; `set_model` / `set_thinking_level` before `stream()`.
- Error modes: `AgentError::{Provider(ProviderError), NoProvider, Cancelled}`; `ProviderErrorKind` is `Config` (permanent, ours) or `Upstream` (transient, theirs).

```rust
// crates/ai-player — the whole surface the app sees
impl Guide {
    pub fn from_env() -> Self;                        // the product's assembly; never fails
    pub fn new(agent: Arc<Mutex<Agent>>) -> Self;      // the seam tests and embedders use
    pub async fn create_session(&self) -> Result<String, AgentError>;
    pub fn end_session(&self);
    pub fn interrupt(&self, id: &str) -> bool;
    pub async fn send(&self, id: &str, game: &Game, req: SendRequest)
        -> Result<(String, impl Stream<Item = Result<StreamChunk, InterruptReason>> + Send), SendError>;
}
pub struct SendRequest { pub input_mode: InputMode, pub thinking_level: ThinkingLevel,
                         pub image_data_url: Option<String> }
pub enum SendError { UnknownSession, Busy, ModeMismatch { bound: InputMode, requested: InputMode },
                     Prepare(AgentError) }
```

- Invariants: one live Session at a time; the InputMode is bound by the first committed Turn; a new session replaces the live one and cancels its in-flight Send; `/ai/...` never mutates the Game. `from_env()` never fails — an unconfigured environment yields a `Guide` whose Load fails with `ProviderErrorKind::Config`, so the player reads it at session creation instead of at startup. `MODEL` returns to `pub(crate)`.
- Ordering: `create_session` → `send`* → `end_session`; `interrupt` affects only that id's in-flight Send.
- Error modes: unchanged — `Prepare` is the only `SendError` carrying a Provider body, returned only before any content and committing no Turn; the app maps the four variants onto HTTP statuses.

```ts
// app ↔ frontend — the wire mirrors, hand-written on both sides
// game:      app's wire.rs DTOs         ↔ frontend/src/game/api.ts
// ai-player: the context's serde types  ↔ frontend/src/ai-player/api.ts
// Invariants: both mirrors must stay byte-compatible with their Rust side; the
// SSE GuideEvent shape belongs to ai-player; the /ai/... paths are the
// transport's names (the guide path rename is #129).
```

### Testing Decisions

No new module is introduced, so no test module is added: every existing suite moves with its module — `core/tests.rs` → `crates/game/src/tests.rs`, the `#[cfg(test)]` blocks in `ai/agent`, `ai/provider/deepseek`, `ai_adapter`, `server/{mod,wire,ai_routes}`, and `main.rs`'s CLI tests. The gate becomes `cargo test --workspace`; a root package alone would silently skip the members.

Prior art the tests already follow: `#[cfg(test)] mod tests` co-located at the bottom of each module; `MockProvider` (`ai/provider/mock.rs`) for the agent seam; `guide_with_mock()` in `ai_adapter`'s tests for the ai-player seam; `makeGameSnapshot` for the frontend; co-located `*.test.ts` beside each frontend module.

`Guide::from_env()` gets no unit test: it reads the environment, and the behaviour it exposes is already covered through `Guide::new` by the mock-based tests. See Further Notes for the split that would make its half testable.

The crate boundary itself is verified by the build rather than by a test: `crates/agent` has no `game` dependency, so "the agent cannot see Minesweeper" is a Cargo fact.

## Out of Scope

- #136 — the agent owning the Session lifecycle (the runtime refactor).
- #129 — retiring the `guide` concept (types, routes, `PlayModeName`, copy).
- #137 — the frontend's off-glossary nouns (`conversation`, `Harness`, `analysis`).
- Re-cutting the transport: no context gets its own routes or DTOs; `game`'s DTOs stay in the app, and ai-player's serde types stay the wire contract. The alternative is its own ADR.
- Redesigning the dormant tool-loop seam (`GameHandle` + `tools()`).
- Any behaviour change.

## Further Notes

- `Guide::from_env()` has no pure half to test. `from_env()` could delegate to a `Guide::from_config(Option<DeepSeekConfig>)`, which would make "an unconfigured environment fails Load with `Config`" a unit test. That line was not settled in the design; the implementation round should decide it rather than add it silently.
- #137's Desired outcome says the dialog renderer is "named from the ai-player glossary", but Dialog is deliberately not a glossary term — the rename follows the code's `dialog*` vocabulary instead. The issue text may want a comment recording that.
- The `game` crate's doc says "no UI, framework, network, or serde dependencies" while it does use `tracing`. Unchanged by this work; noted so a later reader does not read it as a defect introduced here.
