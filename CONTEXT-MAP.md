# Contexts

A context is a language boundary, not a directory: it can span both stacks (`game`, `ai-player`), sit on one stack only (`agent` is Rust-only), or sit beside the code as the root package (the app). Each context owns its glossary, and each glossary lives beside that context's code.

| Context | Rust | TypeScript | Glossary |
| --- | --- | --- | --- |
| `game` | `crates/game` | `frontend/src/game` | `crates/game/CONTEXT.md` |
| `agent` | `crates/agent` | — | `crates/agent/CONTEXT.md` |
| `ai-player` | `crates/ai-player` | `frontend/src/ai-player` | `crates/ai-player/CONTEXT.md` |
| app (root) | `src/` (binary, transport) | `frontend/src/app`, `frontend/src/infra` | this file |

`game` and `agent` depend on nothing above them; `ai-player` depends on both; the app depends on all three. The app is the **composition root**, not a context: it fixes how a Game is launched, serves the transport, and mounts the page layout. Frontend slices never import each other — only `frontend/src/app` imports more than one.

## App-level language

**SessionBox**:
The page's panel that shows an Agent's Session, beside the dashboard in the AI column. It is hidden while no Session is live (the Session's `none` state) and shown from the moment one exists.
_Avoid_: conversation, dialog, chat, transcript
