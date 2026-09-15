# The Agent owns the Session lifecycle — spec

## Problem

The agent runtime is stateless: `Session` is a shared `Vec<Message>` handle, and every lifecycle concern — the id, the in-flight cancel token, Load before the first Send, which run may commit — lives one layer up in the ai-player binding (`SessionBinding`, `InFlight`). The agent glossary already attributes Session, Send, Prepare, Turn, Load and Interrupt to the agent context, so the names and the code disagree on the day that glossary lands.

The runtime cannot drive a conversation on its own: only `complete_once` is self-contained, and it carries no session, so any second consumer — an example, a test, a future headless harness — would have to reimplement the bookkeeping. ADR-0013 kept `ai` separate partly so it could back a headless harness someday; that claim is not true yet.

Constraints: the agent stays blind to Minesweeper (`game` is not one of its dependencies); the ai-player keeps the board payload, the System prompt, the tool wiring and the policy; the transport stays a thin adapter over the domain (ADR-0003). The InputMode binding is not touched here — that is #143.

This supersedes ADR-0017's placement of the session, amends ADR-0018's #136 note, and reopens the merge ADR-0014 deferred (ADR-0022).

## Functional Design

A consumer that knows nothing about Minesweeper can run a conversation end to end — create a Session, send, stream the reply, interrupt, end — while the AiPlayer is left with only what is specific to this game.

**The Agent owns the live Session.** `create_session` brings the runtime up (Load) and replaces the Session; on a Load failure the live Session is untouched. At most one Session is live per Agent and its handle never leaves the Agent. `end_session` forgets it, `interrupt` cancels the Send in flight, `has_committed_turn` answers whether the live Session holds a Turn yet. A Session carries an id, its committed Turns, and the Send in flight against it.

**One Send.** `send` appends the caller's messages, streams the reply, and commits the Turn when the stream reaches `Done`; an interrupted, failed or unread stream commits nothing. `complete_once` is that same Send aggregated into one Reply. `run_loop` is the dormant tool loop (ADR-0013, ADR-0020).

**One failure vocabulary.** `SendError` records why a Send produced no Turn: `NoSession` and `Busy` are refusals before the exchange starts, `Interrupted` is the caller's own act, `Provider(ProviderError)` is the provider's failure — the same variant at delivery and mid-stream, carrying the cause intact. Only the caller's own act is an Interrupt.

**The AiPlayer seam is session-free.** `begin` / `end` / `send` / `interrupt`; the AiPlayer never names a Session. It keeps what is policy — one Game drives one Agent, the InputMode lock, a New Game ending the Session — and it keeps Prepare: the board payload (BoardView), the System prompt for the first Turn, the verbatim player message echoed back for the frontend, and the InputMode check.

**The transport loses the id.** `POST /ai/begin` replaces the live Session and answers 204; `POST /ai/send` either refuses (a status and a body) or streams; `POST /ai/interrupt` answers 204 whether or not a Send was in flight. A stream that stops early says why with one of two events: `interrupted` (the caller's act) or `provider_error` (the provider's failure, in the same body shape the refusal path uses).

**The frontend carries the same vocabulary.** `AiPlayerPhase` becomes `idle` / `running` / `done` / `interrupted` / `failed`, and a failure is a `SendFailure` — `provider` or `refused`. `sessionState` (`none` / `empty` / `non-empty`) and the discard confirm are unchanged.

## Code Design

### Seams

**`crates/agent` ↔ its consumer** — the existing crate boundary (ADR-0018; Cargo enforces that the agent cannot see `game`). It sits there because the mechanism now has one owner, and the Session no longer crosses out of it: the consumer addresses the Agent, never a Session. This is the seam a headless harness or an example consumes.

**`crates/ai-player` ↔ the app** — the existing crate boundary. It sits there because the app is the composition root (ADR-0018) and owns the transport, while the board payload, the System prompt and the policy stay in this crate.

**The wire ↔ the frontend** — served by the app, consumed by the frontend's `ai-player` slice (ADR-0011). It sits there because the app owns the HTTP/SSE shapes and the frontend owns the player-facing phases.

Three seams; no new one is introduced. The `provider` seam inside the agent (ADR-0013) is unchanged, and the tool seam (`Tool`, `add_tool`, `GameHandle`, `tools()`) stays dormant.

### Interfaces

**Seam 1 — `crates/agent`**

```rust
pub enum SendError {
    NoSession,                // refused: no live Session
    Busy,                     // refused: a Send is already in flight
    Interrupted,              // the caller interrupted the Send
    Provider(ProviderError),  // the provider/model exchange failed
}

pub struct ProviderError { kind: ProviderErrorKind, code: Option<u16>, message: String }

impl Agent {
    // construction-time configuration, before the Arc; never mutated later
    pub fn new(providers: ProviderSet) -> Self;
    pub fn set_model(&mut self, model: String, provider: Option<&str>);
    pub fn add_tool(&mut self, tool: Arc<dyn Tool>);

    pub async fn create_session(&self) -> Result<(), ProviderError>;
    pub fn end_session(&self);
    pub fn interrupt(&self) -> bool;
    pub fn has_committed_turn(&self) -> bool;   // false when there is no live Session

    pub async fn send(&self, pending: Vec<Message>, level: ThinkingLevel)
        -> Result<impl Stream<Item = Result<StreamChunk, SendError>> + Send + use<>, SendError>;
    pub async fn complete_once(&self, pending: Vec<Message>, level: ThinkingLevel)
        -> Result<Message, SendError>;
    pub async fn run_loop(&self, pending: Vec<Message>, level: ThinkingLevel)
        -> Result<Message, SendError>;
}
```

The crate re-exports `Agent`, `ProviderSet`, `SendError`, `ThinkingLevel` and `Tool`, and no `Session`: `AgentError`, `InterruptReason`, `set_thinking_level`, `current_provider` and `current_model` are deleted. `load`, the internal stream engine and the `Session` type become crate-internal.

Invariants: at most one live Session per Agent, and it never leaves the crate; a Session holds committed Turns only, plus the Send in flight against it; one Send has one cancel token spanning the whole call, including every provider exchange of a tool loop; the Turn commits atomically under one lock when the stream reaches `Done`; no provider, an unknown model and a serialization failure are all `ProviderError` with `kind: Config`, formed at the source rather than translated by a consumer.

Ordering: `create_session` is Load, then on success replace the live Session and cancel the previous Send; `send` is the admission checks (`NoSession`, then `Busy`), then register the cancel token, then append, stream and commit.

Error modes: `SendError`'s variant set is position-restricted — `NoSession` and `Busy` appear only in `send`'s return value, `Interrupted` only as a stream item, and `Provider` in both. The compiler cannot enforce this, so it is documented on the type.

Known accepted gap: `has_committed_turn()` and `send()` are not atomic. A concurrent `create_session` between them can land a System-less first Turn in a fresh Session; the id-based re-check that closes this today (`crates/ai-player/src/lib.rs:429`) is removed with the id. #143 removes the mechanism, so the gap is accepted — see Out of Scope.

**Seam 2 — `crates/ai-player`**

```rust
impl AiPlayer {
    pub async fn begin(&self) -> Result<(), ProviderError>;
    pub fn end(&self);
    pub async fn send(&self, game: &Game, req: SendRequest)
        -> Result<(String, impl Stream<Item = Result<StreamChunk, SendError>> + Send + use<>), SendError>;
    pub fn interrupt(&self) -> bool;
}

pub enum SendError {
    ModeMismatch { bound: InputMode, requested: InputMode },
    Agent(agent::SendError),
}
```

`SendRequest` keeps its shape (`input_mode`, `thinking_level`, `image_data_url`); the app aliases the two `SendError`s in scope.

Internals: the AiPlayer holds one `Arc<Agent>` and its InputMode lock — `ModeLock { mode: Option<InputMode>, generation: u64 }` behind one `StdMutex` inside an `Arc`. A private `prepare` carries the glossary's Prepare: the mode check, `BoardView::from_game`, the System prompt when the Session has no Turn, the image side effect, the echoed player message, and the mode this Send runs under. A stream guard sets the mode on `Done`, and only when `lock.generation` still equals the generation the Send captured.

Invariants: the AiPlayer never holds a Session; `mode` is set by the Session's first committed Turn and cleared whenever the live Session is replaced; a stale stream can never bind the mode; `ModeMismatch` is checked before the agent sees the Send, so a permanent refusal precedes a transient one; a Load failure in `begin` leaves both the live Session and the mode lock untouched.

Ordering: `begin` awaits `agent.create_session()`, and only on success clears `mode` and bumps `generation` under the same lock — so a guard racing from the previous generation settles either way.

Error modes: `ModeMismatch` is the AiPlayer's only own refusal; everything else is the agent's, wrapped as `SendError::Agent`.

**Seam 3 — the wire**

```
POST /ai/begin      → 204
                    | <status> {kind,code,message}          (Load refused the runtime)
POST /ai/send       → 200 text/event-stream                  (first event: user)
                    | <status> {kind,code,message}           (a provider failure)
                    | <status> {error:string}                (a refusal: NoSession / Busy / ModeMismatch)
POST /ai/interrupt  → 204
```

SSE events: `reasoning` / `content` / `user` / `[DONE]` / `interrupted` / `provider_error{error}`.

Status: a provider `code` is used when it parses, otherwise `Config → 503` and `Upstream → 502`. Refusals: `ModeMismatch` 400, `Busy` 409, `NoSession` 409. The image-mode check (`image mode requires image_data_url` → 400 before the Session is touched) stays as it is.

### Testing Decisions

**`crates/agent`** (`src/agent/mod.rs`, prior art: `MockProvider` with `mock.last_request()`, the inline `ErroringProvider`)

- Rewrite the eleven live ones (`complete_once_*`, `stream_commits_*`, `stream_yields_reasoning_content_then_done`, `cancelled_token_reports_cancelled`, `unknown_provider_is_no_provider`) onto `(pending, level)`, asserting commits through the live Session; the test module is a child of the same file, so a `pub(crate)` read is enough.
- Add: `NoSession` / `Busy` refusals; `end_session` cancels the in-flight Send; a mid-stream `Provider(pe)` keeps its `kind`, `code` and `message` instead of collapsing into a bucket.
- Add the goal test: a consumer holding no Session runs create → send → stream → interrupt → end.
- Delete the `AgentError::Cancelled` assertions.

**`crates/ai-player`** (prior art: `ai_player_with_mock`, `FailingProvider`)

- Rewrite: `send_streams_*`, `the_first_committed_send_binds_the_input_mode`, `a_committed_send_appends_*`, `an_interrupted_first_send_*`, `send_while_a_send_is_in_flight_is_busy`, `interrupt_without_an_in_flight_send_is_false`, `end_session_forgets_the_live_session`, `image_mode_sends_the_screenshot_turn`, `create_session_without_a_provider_is_no_provider`, `a_failed_load_keeps_the_previous_session_live`.
- Add the generation guard test: a superseded stream's `Done` does not bind the new Session's mode (prior art: `create_session_cancels_the_previous_send`).
- Delete `create_session_returns_a_fresh_id_each_time`, `send_for_a_replaced_session_is_unknown_session`, and the four refraction tests (they move to the agent).
- Untouched: `send_payload_does_not_leak_hidden_mines` (the privacy hard constraint), `persist_image_*`, `civil_from_days_matches_the_unix_epoch`, `current_timestamp_is_yyyymmdd_hhmmss`.

**`src/server`** (`ai_routes` tests)

- Rewrite the status assertions (`NoSession` is no longer a 404; `interrupt` is always 204), add `/ai/begin` 204 and the 503 / 502 rule, and assert the two new SSE events.
- Keep `send_future_is_send` (axum needs the future to stay `Send`).

**`frontend`** (vitest, prior art: the four existing `*.test.ts` in `src/ai-player` and `src/app`)

- `api.test.ts`: the three route strings, no `sessionId`, parsing of `interrupted` and `provider_error`.
- `stateMachine.test.ts`: `begin()` failing into `failed` with a `provider` failure; `interrupted`; `refused`.
- `sessionBox.test.ts`: the interrupt line without a reason. `aiPlayMode.test.ts`: the alert fires only for `failure.kind === "provider"`.

## Out of Scope

- **#143** — binding the InputMode and the System prompt when the Session is created. When it lands, the Send body loses `input_mode`, `ModeMismatch` goes, and the `has_committed_turn` / `pending` shape shrinks. The known accepted gap above exists only until then.
- **The DeepSeek adapter's mid-stream detail** — `SseState` only ever emits `{Upstream, code: None}`, so a real mid-stream 429 or 5xx still reads as a timeout. This work removes the lossy refraction, not the adapter's loss.
- **The tool loop** — `run_loop` stays dormant and is reshaped only enough to compile; ADR-0020 governs its rework.
- **The interrupt UI** — the presentation of `interrupted` and of a mid-stream provider failure (the #97 placeholder).

## Further Notes

The session's mechanism is a move, not a rewrite: the commit rule, the cancel semantics, the Load-before-first-Send ordering and the "an interrupted Send adds no Turn" rule are all as they are today — they change owner, and the failure vocabulary around them is unified.

This work carries its documentation with it: `crates/agent/CONTEXT.md` (Agent, Session, Send rewritten; `InterruptReason` replaced by `Interrupt`), `crates/ai-player/CONTEXT.md`, ADR-0021 and ADR-0022, and the supersede notes on ADR-0014, ADR-0017 and ADR-0018.
