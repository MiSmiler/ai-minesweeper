## Toolchain

- **Backend** (`src/`): Rust (edition 2024) + axum, built and tested with `cargo` (`cargo fmt`, `cargo test`).
- **Frontend** (`frontend/`): Vite + TypeScript app, built with `npm run build` (tsc + Vite), tested with `vitest`, formatted with `prettier`.
- **Playwright** (`frontend/` devDependency): a headless browser you can drive to **screenshot the web UI** with no repo wiring — launch `chromium` from a one-off node invocation, load the app (a running `cargo run` backend on port 8080, or the Vite dev server via `npm run dev`, default `:5173`), and save screenshots to `.scratch/shot/` (gitignored). Live state is at `GET /state`, actions at `POST /action`.

## Development habits

### Before modifying code

When investigating a problem (debugging, diagnosing an error, exploring the codebase), **report findings first and wait for confirmation** before making any changes. Do not jump from investigation directly into editing.

### Interface design alignment

Before implementing any feature or fix — any change to `src/` or `frontend/`
that writes code — **align the interface design with the user**: lay out the
interfaces and the shape of the change, and wait for confirmation before
touching a file. The implementation is not underway until the design is agreed.

### After a coding round

A round of code changes (feature, fix, or a batch of edits) is not done until the formatting and tests for the sides of the stack it touched pass:

**Rust** (`src/`):
1. Run `cargo fmt` to format the code.
2. Run `cargo test` and ensure everything passes.

**Frontend** (`frontend/`):
1. Run `npm run format` to format the code.
2. Run `npm test` and ensure everything passes.
3. Run `npm run build` (tsc type check + Vite build) and ensure it passes.

If a failure is clearly caused by your changes, fix it before reporting done. If the cause is unclear or unrelated, report it and wait for confirmation first.

Skip a side's steps when the round touched no files of that side; skip everything when the round touched only non-code files (docs, markdown, etc.).

### Before committing

Before executing `git commit`, **show the proposed commit message and wait for confirmation**. Use Conventional Commits format (`feat:`, `fix:`, `docs:`, `chore:`, etc.).

## Rust conventions

Write a constructor — `fn foo(...) -> SomeType` that builds `SomeType` — as an associated
function on `SomeType`, placed next to its definition (e.g. `game_config(...) -> GameConfig`
is `GameConfig::new(...)`). Before writing any free function, ask: "what type is this about?"

## Agent skills

### Issue tracker

Issues live in the repo's GitHub Issues and are managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage labels use the default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context repo — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
