# ai-minesweeper

A Minesweeper game with a web UI: a TypeScript frontend over a Rust backend, controlled entirely by mouse. The backend owns the game state and serves the built frontend plus a small JSON action API; the browser is a thin client that sends actions and renders state.

## Running

Requires Rust (edition 2024) and a built frontend.

```bash
# Build the frontend once (the server serves frontend/dist)
cd frontend
npm install
npm run build
cd ..

# Run the server — the UI is served at the printed URL
cargo run
```

| Flag | Meaning | Default |
| --- | --- | --- |
| `--seed <seed>` | Pin one Seed — a plain `u32` — for every game of this session: each Difficulty reproduces the same Mine layout. Absent, every New Game draws a fresh random Seed, printed to the terminal. | random |
| `--prank` | Prank Mode: the First Click of every game is always a Mine. The UI never indicates the mode is active. | off |
| `--port <u16>` | Port to listen on. | 8080 |
| `--host <string>` | Interface to bind. | 127.0.0.1 |

Frontend development uses Vite's own dev server: `cd frontend && npm run dev` and open the URL it prints. `npm test` runs the Vitest suite; `cargo test` runs the Rust suite.

## Log levels

- **Backend** (Rust, `tracing`): defaults to `info` — startup (URL, mode), New Game (Seed, difficulty), and Won/Lost are visible. `debug`/`trace` messages (e.g. "action applied") need `RUST_LOG=debug` (or `RUST_LOG=ai_minesweeper=debug`) to appear.
- **Frontend** (TypeScript, `tslog`): dev builds (`vite dev`) log `debug` and above — including the chord gesture state-transition trace (armed / preview-set / preview-moved / preview-cleared / chord / disarmed); production builds (`vite build`) log only `warn`/`error`; tests are silenced. Any non-test build can be re-gated at build time with `VITE_LOG_LEVEL=<name> npm run build`, where `<name>` is one of `silly`, `trace`, `debug`, `info`, `warn`, `error`, `fatal`.
