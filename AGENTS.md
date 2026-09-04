## Toolchain

- **Backend** (`src/`): Rust (edition 2024) + axum, built and tested with `cargo` (`cargo fmt`, `cargo test`).
- **Frontend** (`frontend/`): Vite + TypeScript app, built with `npm run build` (tsc + Vite), tested with `vitest`, formatted with `prettier`.
- **Playwright** (`frontend/` devDependency): a headless browser you can drive to **screenshot the web UI** with no repo wiring — launch `chromium` from a one-off node invocation, load the app (a running `cargo run` backend on port 8080, or the Vite dev server via `npm run dev`, default `:5173`), and save screenshots to `.scratch/shot/` (gitignored). Live state is at `GET /state`, actions at `POST /action`.

## Development habits

### Before touching files: present first, then confirm

Before you edit any file, present what you're about to do and **wait for the
user's confirmation**. The form of that presentation depends on what the user
asked:

- **A question** ("what do you think?" / "why?" / any `?`): it is a request for
  analysis and explanation, **not** a cue to edit. Give the analysis first; edit
  only after the user confirms they want the change made.
- **Investigating a problem** (debugging, diagnosing, exploring the codebase):
  report your findings first; do not jump from investigation straight to editing.
- **Implementing a feature/fix** (a change to `src/` or `frontend/`): lay out
  the interface design and the shape of the change first; the implementation is
  not underway until the design is agreed.

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

### Git staging

**Never unstage on your own** (`git reset`, `git restore --staged`, or any way that moves previously staged work back out of the index) unless the user explicitly instructs it. If you believe unstaging is genuinely necessary mid-round, **ask for the user's explicit consent before running it.**

### Before committing

Before executing `git commit`, **show the proposed commit message and wait for confirmation**. Use Conventional Commits format (`feat:`, `fix:`, `docs:`, `chore:`, etc.).

## Agent skills

### Issue tracker

Issues live in the repo's GitHub Issues and are managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage labels use the default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context repo — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
