# Frontend organized by feature slice, layered within each slice

The frontend `src/` was flat — every TypeScript module sat at the top level (`api.ts`, `client.ts`, `controller.ts`, …). The backend `src/` is already grouped by concern (`core/` for pure rules, `server/` for the transport layer), and the frontend was growing past the point where one directory made each module's role obvious. The frontend is also heading toward multiple play modes (human-plays, AI-watches, AI-plays, AI-and-human) behind a shared top bar, which will need reusable, instantiable units — two boards side by side in the AI-and-human mode, an AI message stream, a mode switcher.

We organize the frontend **by feature slice first, then by layer within each slice**, mirroring the backend's layer spirit without committing to a single flat layer set across the whole app.

```
frontend/src/
├── app/        top-level assembly: the top bar, the mode switcher (future), main.ts
├── game/       feature slice: Minesweeper
│   ├── api.ts      wire contract (mirrors the server DTOs)
│   ├── client.ts   the game orchestration (factory: createGameClient(deps))
│   ├── logic/      pure logic, no DOM (gesture, controller, chordPreview, hitTest geometry)
│   └── render/     DOM rendering (render, previewHighlight, hitTest measurement)
├── infra/      shared infrastructure (log, testUtils)
└── ai/         future slice: AI conversation — created lazily when AI code lands
```

Each slice owns its contract, logic, render, and orchestration. Modes are compositions performed in `app/` — a mode recipe picks the slices it needs (game board, AI message box, parameter panel) and wires them together.

Considered options:

- **Pure horizontal layers** (a single `contract/`, `logic/`, `render/` across the whole app, the literal reading of the backend's `core/`+`server/`): clean today, but a future vertical capability like AI would shatter across the shared layers — its logic, render, and contract would sit intermixed with the game's. This is exactly the fear behind the issue's "will AI make this messy?".
- **Pure feature folders** (one directory per feature, every feature a vertical slice): closer to the domain, but today's modules are small — many directories would hold a single file — and it diverges from the backend's layer reading.
- **Shallow/two-directory** (`logic/`, `render/`, rest at root): minimal change, but the orchestration center (`client.ts`) stays a root-level oddity and it does not meet the issue's "structured like `src/`" goal.

We chose feature-slice-plus-layers because it keeps the per-layer readability of the backend while giving each vertical capability (game, AI) a home that can grow without colliding with the others; `app/` is where modes get composed. Today the only non-empty slice is `game/`, so the layout reads like a small layered tree — the `ai/` slice is deliberately not pre-created.

Consequences and boundaries:

- **Issue #109 scope is "move files, change no logic."** The directory tree is a relocation; module behavior, tests, and interfaces are untouched.
- `hitTest.ts` mixes pure geometry (`offsetToIndex`, `BoardGeometry`) and DOM measurement (`measureBoard`, `cellAtPoint`). It is intentionally left as one unit, so it lands in `game/render/`; the split was considered and decided against.
- Cross-slice imports use relative paths; no path aliases (the tree is small, and aliases add indirection without a payoff).
- Tests stay co-located with their source (`.test.ts` beside the module), matching the pre-existing convention.
- `style.css` and `vite-env.d.ts` stay at `src/` root: they are not modules and are not grouping objects.
- The `game/client.ts` factory (`createGameClient(deps)`) means the game slice is already instantiable — the AI-and-human two-board mode (two independent boards) needs no structural rework, only the later multi-instance wiring.
