---
name: repro-by-playwright
description: Repro a Minesweeper game scene with Playwright and screenshot it. Use when a task is about how the game looks or how a gesture behaves (a Press/Chord Preview following the pointer, the Smiley face, the Won/Lost reveal, a missed reveal or flag), or when a human report says "reproduce this scene". Drives real mouse events via frontend/scripts/shot.mjs and saves screenshots to .scratch/shots/.
---

# Repro a game scene with Playwright

When a task is about how the game **looks**, or about a **gesture** (a Press/Chord Preview following the pointer, the Smiley face, the Won/Lost reveal, a missed reveal or flag), reason from a screenshot instead of from code alone. The helper in `frontend/scripts/shot.mjs` injects the real mouse events the game listens to and screenshots the result to `.scratch/shots/`.

## When to reach for it

- A human report says "reproduce this scene".
- The issue is visual / rendering / gesture-pipeline rather than pure logic.

## Run a scene script

1. Ensure a game service is reachable. The script **reuses** one already serving `GET /state` on `SHOT_BASE_URL` (default `http://127.0.0.1:8080`), or starts `cargo run` itself. A Vite dev server works too via `SHOT_BASE_URL=http://localhost:5173`.
2. Write a scene script importing `openshot` from `../shot.mjs` (relative to `frontend/scripts/`), or start from an example:
   - `frontend/scripts/examples/basic.mjs` — reveal + screenshot.
   - `frontend/scripts/examples/chord.mjs` — pick a revealed numeric Cell from `snapshot()` and chord it.
3. Run from `frontend/`: `node scripts/<my-scene>.mjs [seed]`.
4. **Done when** the script prints the saved screenshot path, and the PNG shows the expected board state (open it with `read`).

## API (`shot.mjs`)

- `openshot({seed, prank, base})` → opens a session (reused service or self-started).
- `reveal(row, col)` — Left press + release.
- `flag(row, col)` — Right press on a **non-revealed** Cell.
- `hover(row, col)` — move the pointer (drives Press/Chord Preview follow).
- `chord().pressAt(r, c).moveTo(r2, c2).release()` — chainable; both presses must land on a revealed Cell to arm.
- `snapshot()` — `GET /state`.
- `newGame(difficulty?)` — restart.
- `screenshot(name)` — writes into `.scratch/shots/`.
- `shot.page` — the Playwright page, for anything beyond the helpers.

## Gesture semantics to get right

- `reveal` fires on Left release, over the Cell the pointer is on (the press-preview Cell).
- `flag` fires on Right **down** on a non-revealed Cell.
- a `chord` arms only when Left and Right are both pressed on a Revealed Cell; releasing Left solves the Cell **under the pointer** (the Chord Preview position) — so a chord can press on one Cell and release on another.

## Seed

`--seed` pins the board only when the script starts its own backend. A **reused** service keeps its running layout; the script prints a note when `--seed` is ignored (the running service's layout applies).

## Gotchas

- The chord builder's stage methods (`pressAt`/`moveTo`) only **enqueue** a mouse step; `release()` runs them in order. Chain them (or call them all) before `release()` — they do not act by themselves.
- `waitUntil: "networkidle"` would hang — the frontend polls `/state` every second; the script uses `domcontentloaded`.
- Screenshots land in `.scratch/shots/` (gitignored); open them with the `read` tool to look.
