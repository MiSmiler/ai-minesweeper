# Contexts

A context is a language boundary, not a directory: it can span both stacks (`game`, `ai-player`), sit on one stack only (`agent` is Rust-only), or sit beside the code as the root package (the app). Each context owns its glossary, and each glossary lives beside that context's code.

| Context | Rust | TypeScript | Glossary |
| --- | --- | --- | --- |
| `game` | `crates/game` | `frontend/src/game` | `crates/game/CONTEXT.md` |
| `agent` | `crates/agent` | — | `crates/agent/CONTEXT.md` |
| `ai-player` | `crates/ai-player` | `frontend/src/ai-player` | `crates/ai-player/CONTEXT.md` |
| app (root) | `src/` (binary, transport) | `frontend/src/app`, `frontend/src/infra` | this file |

`game` and `agent` depend on nothing above them; `ai-player` depends on both; the app depends on all three. The app is the **composition root**, not a context: it fixes how a Game is launched, serves the transport, and mounts one PlayMode at a time. Frontend slices never import each other — only `frontend/src/app` imports more than one.

## App-level language

**PlayMode**:
The perspective a single Game is shown from, independent of the rule set and of which Features are on: `HumanPlay` — the HumanPlayer plays unaided; `AiPlay` — an AiPlayer plays the Game, driven by hand for now. The set is open and may grow (e.g. `HumanVsAiPlay`); a Game has exactly one PlayMode at a time.
_Avoid_: PlaySurface, AiGuide, AiHelpMePlay, view, perspective (when meaning the mode)

**Composition**:
The app's mounting of one PlayMode: the assembled game area, dashboard and SessionBox for that mode, with its own teardown. The app holds one Composition at a time; switching PlayMode disposes the current one and mounts a new one.
_Avoid_: view, screen, layout

**SessionBox**:
The panel of a Composition that shows an Agent's Session.
_Avoid_: conversation, dialog, chat, transcript
