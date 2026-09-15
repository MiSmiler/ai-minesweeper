# Retire the guide concept: the AI mode is the AiPlayer playing — spec

## Problem

The AI mode was framed as an AI watching a human play, and the whole AI path took its names from that framing: the `Guide` object, the `AiGuide` PlayMode, the `/ai/guide/...` routes, the `Guide*` types, the `guide`-named files and CSS classes, and the docs. The product direction is the other way round — the AI plays the Game — and the interim manual interaction (the player sends the Board and applies the reply) is a driver of that one mode, not a mode of its own.

The language has also drifted from the code: `CONTEXT-MAP.md` and ADR-0013 define the second PlayMode as `AiHelpMePlay`, while `frontend/src/app/mode.ts` and the code comments call it `AiGuide`.

Two facts found while designing this, both relevant beyond the rename:

- The wire event union is mostly agent-shaped. `reasoning` / `content` / `sse_done` project `agent::StreamChunk`; `interrupt` projects `InterruptReason` — which `crates/agent/CONTEXT.md` already claims while `crates/ai-player/src/lib.rs` still defines it. Only `user`, the rendered payload echo, is the ai-player's. That disagreement belongs to #136, which is **not** touched by this work.
- The domain relation "one Send commits one `user` / `assistant` pair" holds today because a Send is one provider exchange. Naming the assistant half **Reply** makes that relation explicit and makes the tool loop's effect on it visible.

This work changes no behaviour: it renames, defines, and records.

## Functional Design

### Vocabulary

`crates/game/CONTEXT.md` gains the two parties. **Player** goes right after **Game**, since a Game's definition already speaks of the player.

```md
**Player**:
One of the parties playing a Game: a HumanPlayer or an AiPlayer. A Player Reveals Cells, places Flags and Chords.
_Avoid_: user, participant, actor, opponent

**HumanPlayer**:
A Player who plays by hand, through the Board's Gestures with the mouse.
_Avoid_: user, the human, real player
```

`HumanPlayer` is a glossary concept with no code yet. It is minted now because the `HumanVsAiPlay` PlayMode will need both parties named at once, and because a `Player` umbrella whose only member is `AiPlayer` reads as if the human were not one.

`crates/ai-player/CONTEXT.md` gains **AiPlayer** as its first entry.

```md
**AiPlayer**:
A Player that plays a Game by deciding its moves with an Agent. One AiPlayer exists per app instance, and today it is driven by the HumanPlayer, who sends the Board and applies the reply.
_Avoid_: AI, agent, bot, AI opponent, guide, advisor
```

The definition deliberately does not name the fields `AiPlayer` holds: #136 moves the Session lifecycle into the `Agent`, after which the binding keeps only this game's policy.

`crates/agent/CONTEXT.md` gains **Reply** after **Turn**. `Turn`, `Session`, and `Send` keep their current bodies — the tool loop's effect on them is recorded in ADR-0020 instead.

```md
**Reply**:
The assistant's half of a Turn — the Agent's answer to a Send.
_Avoid_: response, answer, completion, message
```

`CONTEXT-MAP.md`'s **PlayMode** is rewritten, which also fixes the `AiHelpMePlay` drift. Its `_Avoid_` now carries the two retired names.

```md
**PlayMode**:
The perspective a single Game is shown from, independent of the rule set and of which Features are on: `HumanPlay` — the HumanPlayer plays unaided; `AiPlay` — an AiPlayer plays the Game, driven by hand for now. The set is open and may grow (e.g. `HumanVsAiPlay`); a Game has exactly one PlayMode at a time.
_Avoid_: PlaySurface, AiGuide, AiHelpMePlay, view, perspective (when meaning the mode)
```

`advisor` is retired alongside `guide`: it is another non-term name for the same thing, in `src/server/mod.rs` (2 lines), `crates/ai-player/src/lib.rs` (4), and `src/server/ai_routes.rs` (1). Those seven comments are rewritten in terms of `AiPlayer`. Its occurrences in ADR-0013 and `crates/ai-player/docs/adr/0017` are historical text and stay.

### ADRs

New **ADR-0019** (root, since it covers the wire and the app-level PlayMode):

```md
# The AI mode is the AiPlayer playing; the manual Send is a temporary driver

ADR-0012 defined the second PlayMode as `AiHelpMePlay` — an AI watching a human play — and the AI path took its names from that framing (`Guide`, `AiGuide`, `/ai/guide/...`). The product direction is the other way round: the AI plays the Game. Today's manual interaction — the player presses Send, the Board goes out as the payload, the player applies the reply — is not a mode of its own but a temporary driver, and it exists for one reason: the AiPlayer has no tools yet (`GameHandle` / `tools()` have no caller — ADR-0018), so the Board must be pushed to the model as a Send. Once the tool loop lands (ADR-0013's `AiPlay`), the AiPlayer reads the Game through its tools, and the per-move Send goes with the dashboard that drives it. We name the party that plays `AiPlayer` and the human side `HumanPlayer` (a glossary concept with no code yet), rename `AiHelpMePlay` to `AiPlay` and `SinglePlay` to `HumanPlay`, and retire the `guide` name everywhere.
```

New **ADR-0020** (root; both `agent` and `ai-player` are concerned, and it must be readable before the tool loop exists):

```md
# Reply is one assistant message

Minting `Reply` next to `Turn` records today's relation: one Send commits one `user` / `assistant` pair, so one Send has exactly one Reply. The tool loop `AiPlay` will run (ADR-0013, ADR-0018: `GameHandle` / `tools()` have no caller yet) breaks that 1:1 — one Send will produce several Replies — while the commit stays where it is today: `Agent::stream` commits `pending` plus the assistant message per provider exchange, so a Send interrupted between exchanges leaves the earlier Replies in the Session. Whoever builds the tool loop must revisit **Turn** (it stops being one Send + one Reply), **Session** (an interrupted Send no longer adds nothing) and **Send** (failing between exchanges is a new case); `Reply` itself does not change.
```

**ADR-0012 is reopened**: its PlayMode set and the meanings of its two members are replaced by ADR-0019. ADR-0013, ADR-0018, and `crates/ai-player/docs/adr/{0014,0015,0017}` get a `Renamed by ADR-0019` line at the top; their decisions stand.

### PlayMode and compositions

| Concept | `PlayModeName` | Composition | Switcher label |
| --- | --- | --- | --- |
| `HumanPlay` | `human` | `humanPlayMode.ts` / `composeHumanPlayMode` | `Human` |
| `AiPlay` | `ai` | `aiPlayMode.ts` / `composeAiPlayMode` | `AI` |
| `HumanVsAiPlay` (future) | — | — | — |

The concept name and the runtime identifier stay un-unified (ADR-0012's deliberate split); `?mode=` accepts only `human` and `ai`, and any other value falls back to `human`. **No compatibility is kept**: the old `/ai/guide/...` routes disappear rather than alias, and the old `?mode=single` / `?mode=ai-guide` values are not mapped. The frontend and the backend ship as one artifact, and the only consumers are the app itself and dev URLs.

The `guide`-prefixed CSS classes are renamed to `.ai-play-*`, matching the composition file that writes them.

### Zero behaviour change

No decision logic, HTTP status, error body, SSE semantic, or user-facing Chinese copy changes — the frontend's Chinese copy is #137's. What changes is names, and the `guide` name no longer appears anywhere in the code.

## Code Design

### Seams

**S1 — HTTP wire (app ↔ frontend)** — `src/server/ai_routes.rs` produces, `frontend/src/ai-player/api.ts` consumes. Both sides are hand-written mirrors of one contract, and the routes are the only place the app exposes the AiPlayer to the browser.

**S2 — `crates/ai-player`'s public API (app → crate)** — `AiPlayer` is the crate's central object; the app reaches the `Agent` and the live Session only through it. The seam already exists; only the type name changes.

**S3 — the frontend's app composition seam + slice seam** — `frontend/src/app/mode.ts` (how a PlayMode is named, mounted, and switched: ADR-0012) and `frontend/src/ai-player/api.ts` (the `AiApi` the compositions are injected with, and its events).

No new seam is introduced, and none moves.

### Interfaces

**S1 — the wire**

```ts
POST /ai/session                       -> 200 { session_id } | ProviderError{kind,code,message} + status
POST /ai/session/{id}/send             body { input_mode, thinking_level?, image_data_url? }
                                       -> 200 text/event-stream
                                          {kind:"user",text}                 ← first
                                          {kind:"reasoning"|"content",text}*  ← interleaved
                                          data: [DONE]                       ← last
                                          | {kind:"interrupt",reason}        ← replaces [DONE]
                                       -> 404 {error} | 409 {error} | 400 {error} | ProviderError
POST /ai/session/{id}/interrupt        -> 204 | 404
```

```rust
// src/server/ai_routes.rs — pub(crate), Serialize only, no Done variant
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ReplyEvent {
    Reasoning { text: String },
    Content { text: String },
    Interrupt { reason: InterruptReason },
    User { text: String },
}
```

```ts
// frontend/src/ai-player/api.ts — the TS twin of the above
export type ReplyEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "user"; text: string }
  | { kind: "sse_done" }                       // synthesized locally on [DONE]; never on the wire
  | { kind: "interrupt"; reason: InterruptReason };
```

- Invariants: the `kind` spellings are identical on both sides; `ReplyEvent` is not the domain type — the domain stream is `Result<agent::StreamChunk, ai_player::InterruptReason>`, which `to_event` projects onto the DTO. `Done` never becomes a variant (`StreamChunk::Done` is written as `data: [DONE]`), and the DTO is `Serialize`-only (the request body is `SendRequest`, a different seam).
- Ordering: the `User` event is chained to the head of the stream, before any agent chunk; `[DONE]` or `interrupt` closes it.
- Error modes: 404 unknown session, 409 a Send is already in flight, 400 mode mismatch or an image Send without `image_data_url`, and the `ProviderError` body on Load and on a failed Prepare — byte-for-byte what the routes return today.

**S2 — `crates/ai-player`'s public API**

```rust
// the whole surface the app sees
pub struct AiPlayer { /* private */ }

impl AiPlayer {
    pub fn new(agent: Arc<tokio::sync::Mutex<Agent>>) -> Self;
    pub fn from_env() -> Self;
    pub async fn create_session(&self) -> Result<String, AgentError>;
    pub fn end_session(&self);
    pub fn interrupt(&self, id: &str) -> bool;
    pub async fn send(&self, id: &str, game: &Game, req: SendRequest)
        -> Result<(String, impl Stream<Item = Result<StreamChunk, InterruptReason>> + Send), SendError>;
}

pub struct SendRequest { pub input_mode: InputMode, pub thinking_level: ThinkingLevel,
                         pub image_data_url: Option<String> }
pub enum SendError { UnknownSession, Busy, ModeMismatch { bound: InputMode, requested: InputMode },
                     Runtime(AgentError) }
```

- Unchanged, and deliberately so: `InputMode`, `BoardView`, `SendRequest`, `SendError`, `InterruptReason`, `GameHandle`, `tools()`. Only `Guide` disappears.
- Invariants: one live Session at a time; the InputMode is bound by the first committed Send; one Send in flight at a time; the payload carries only player-visible state — `BoardView::from_game` is the privacy seam, and it reads only the visible API of `Game`.
- Ordering: `create_session` before `send`; `end_session` clears the binding and cancels the in-flight Send; `/ai/...` never mutates the Game.
- Error modes: `SendError` as above, with `Runtime` being the only variant carrying a Provider body; a Load failure in `create_session` is an `AgentError` that the app maps onto the same Provider body.

**S3 — the frontend's composition seam and slice seam**

```ts
// frontend/src/app/mode.ts
export type PlayModeName = "human" | "ai";

export function mountMode(mode: PlayModeName, root: HTMLElement, deps: AppDeps): Composition;

export interface Composition {
  dispose(): void;
  hasSessionHistory?(): boolean;   // synchronous, side-effect free: read inside beforeunload only
  confirmDiscard?(message: string): boolean;
}

export interface AppDeps {
  getPlayMode(): PlayModeName;
  aiApi: AiApi;
  captureBoardImage: CaptureBoardImage;
}

// frontend/src/ai-player/api.ts
export interface AiApi {
  createSession(): Promise<{ sessionId: string }>;
  send(sessionId: string, req: SendRequest,
       onEvent: (e: ReplyEvent) => void,
       onProviderError: (e: ProviderError) => void): void;
  interrupt_by_user(sessionId: string): Promise<unknown>;
}
```

- Invariants: only the AiPlay composition implements `hasSessionHistory` / `confirmDiscard` — it is the only one holding a Session a refresh or a switch would discard; switching PlayMode always disposes the current composition and abandons the current Game (ADR-0012); `hasSessionHistory` must stay synchronous, dialog-free, and I/O-free, because its only caller is `beforeunload`.
- Ordering: `confirmDiscard` is consulted before `dispose`; a `false` return leaves the current composition mounted and the mode unchanged. On an accepted switch: `dispose` → persist `?mode=` → `mountMode`.
- Error modes: HTTP and Prepare failures reach the composition through `onProviderError` and surface as a blocking alert; `sse_done` is synthesized by `consumeEvents` when it reads `[DONE]`.

### Rename inventory

| Location | Change |
| --- | --- |
| `crates/ai-player/src/lib.rs` | `Guide` → `AiPlayer` (struct, `impl`, every doc reference); test helpers `guide_with_mock` / `guide_with_failing`; local `let guide` bindings |
| `src/main.rs`, `src/server/mod.rs` | `Guide::from_env()`; `AppState.guide` → `AppState.ai_player`; the two `advisor` comments |
| `src/server/ai_routes.rs` | `handle_guide` → `handle_send`; `GuideEventDto` → `ReplyEvent`; the two route paths; the `advisor` comment; doc comments and test helpers |
| `crates/agent/src/agent/mod.rs` | the two `Guide` comment references; the stale `#115+` reference → `(an AiPlay concern)`; the doc wording for one provider exchange: "one turn" → "one Reply" in `stream` / `complete_once` / `run_loop` |
| `frontend/src/ai-player/api.ts` | `GuideEvent` → `ReplyEvent`; `consumeGuide` → `consumeEvents`; the `GuideEventDto` reference; the send URL; the log string `POST /ai/guide/:id/interrupt`; the alert message `AI guide request failed (HTTP …)` → `AI request failed (HTTP …)` |
| `frontend/src/ai-player/stateMachine.ts`, `conversation.ts` | `GuidePhase` / `GuideState` / `GuideMachine` / `createGuideMachine` → `AiPlayerPhase` / `AiPlayerState` / `AiPlayerMachine` / `createAiPlayerMachine` |
| `frontend/src/app/mode.ts` | `PlayModeName` → `"human"` / `"ai"`; switcher labels → `Human` / `AI`; `hasGuideHistory` → `hasSessionHistory`; imports and doc comments |
| `frontend/src/app/main.ts` | the `?mode=` validation list and default; the guard call; the transport comment |
| `frontend/src/app/guideMode.ts`, `singleMode.ts` | → `aiPlayMode.ts` / `composeAiPlayMode`, `humanPlayMode.ts` / `composeHumanPlayMode` |
| `frontend/src/app/*.test.ts`, `frontend/src/ai-player/*.test.ts` | follow the renames: `guideMode.test.ts` → `aiPlayMode.test.ts`, `singleMode.test.ts` → `humanPlayMode.test.ts`, plus fixtures, labels, and asserted URLs |
| `frontend/src/style.css` | `.guide-*` → `.ai-play-*` |
| `frontend/src/game/gameArea.ts`, `frontend/src/game/client.ts`, `frontend/src/ai-player/axis.ts`, `frontend/vite.config.ts` | comments naming `AiGuide` / `guide` |

### Testing Decisions

No new module and no new behaviour, so no new test module: every existing suite follows its module's renames, which is also what verifies the rename stayed mechanical — the Rust suites assert route paths, SSE payloads, and the session lifecycle, and the frontend suites assert the mounted DOM, the switcher, and the fetched URLs.

Two names are deliberately *not* asserted anywhere: `PlayModeName`'s values are covered through `mode.test.ts`'s switch behaviour and `main.test`-less `?mode=` validation; `HumanPlayer` has no code to cover until `HumanVsAiPlay` lands.

Verification for the round: `cargo fmt` + `cargo test --workspace`, `npm run format` + `npm test` + `npm run build`.

## Out of Scope

- **#136**, in full: no comment, no body edit, no issue link. The agent-shaped wire event union found above is recorded in Further Notes for whoever picks it up.
- The tool loop itself, and the `Turn` / `Session` / `Send` glossary bodies it will change. No ticket tracks it today; ADR-0013 and ADR-0018 name it `AiPlay`, and ADR-0020 points there.
- **#137** — the frontend's off-glossary nouns (`conversation`, `Harness`, `analysis`) and the AI panel's Chinese copy.
- Moving `ReplyEvent` into `src/server/wire.rs`, which calls itself the home of the wire contract. Kept where the SSE mapping and its `NewSessionDto` / `ErrorDto` neighbours are.
- The historical `advisor` / `Guide` wording inside ADR-0013 and `crates/ai-player/docs/adr/*`.
- Any behaviour change.

## Further Notes

- The `Dto` suffix is dropped here because it exists to disambiguate from a domain type of the same name: `Cell` / `Position` are `game` types, so `wire.rs`'s mirrors carry it, while `GameSnapshot` has no twin and does not. `ReplyEvent` has no domain twin — `Reply` is a glossary term, not a type — so the suffix buys nothing, and the Rust and TS names match. `NewSessionDto` and `ErrorDto` keep theirs; renaming them is not this work.
- The note that belongs on #136's body, deliberately not filed: the event union is mostly agent-shaped (`reasoning` / `content` / `sse_done` project `agent::StreamChunk`, `interrupt` projects `InterruptReason`, which the agent glossary already claims while `crates/ai-player/src/lib.rs` defines it; only `user` is the ai-player's); the DTO does not move, because the wire belongs to the app and `agent` is Minesweeper-blind; what #136 owns is which crate owns the domain types behind the events.
- `AiPlayer::send` takes a `game` argument for the same reason the manual driver exists: with no tools, the Board has to be pushed to the model. Once the tool loop lands, the Game reaches the AiPlayer through `GameHandle` / `tools()` instead, so both the `game` argument and the per-move call are interim — the tool-loop round should decide their replacement rather than preserve them by default. `Send` itself stays: one Send can still be what starts the loop (ADR-0020).
- `AIApi`'s `interrupt_by_user` is snake_case in otherwise camelCase TypeScript. Unchanged by this work; noted so a later reader does not read it as introduced here.
