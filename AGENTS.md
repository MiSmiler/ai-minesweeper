## Communication Guidelines

Align **one point at a time**. Put a single question or decision to the user, then wait for the answer before raising the next: the reader answers one thing at a time; everything else sits in their head as cognitive load, and the one needing an answer is lost among them. This holds even when a skill's own process calls for a batch of questions.

**Ask before summarising.** When a discussion looks settled, ask whether to summarise and write it only on a yes. An unasked-for summary arrives as a surprise, and can close a branch the user meant to keep open.

## Toolchain

- **Backend** (root package + `crates/*`): Rust (edition 2024) + axum, built and tested with `cargo` (`cargo fmt`, `cargo test --workspace` — with a root package, a plain `cargo test` runs only the root package).
- **Frontend** (`frontend/`): Vite + TypeScript app, built with `npm run build` (tsc + Vite), tested with `vitest`, formatted with `prettier`.
- **Playwright** (`frontend/` devDependency): a headless browser you can drive to **screenshot the web UI** with no repo wiring — launch `chromium` from a one-off node invocation, load the app (a running `cargo run` backend on port 8080, or the Vite dev server via `npm run dev`, default `:5173`), and save screenshots to `.scratch/shot/` (gitignored). Live state is at `GET /state`, actions at `POST /action`.

## Development habits

### Design before code

Rounds of discussion converging is not the design being settled. Both halves — functional design and interface design — are aligned with the user first, and only then does implementation open; a few rounds is the expected cost, not a delay to be cut short.

So when a design discussion has run and you are ready to start coding, the next move is the `confirm-design` skill: it closes both halves with the user before any edit lands. The user reaches for that skill themselves when they see you edging toward the code — the invocation answers "is the design settled?" with *not yet*, so stay in the design rather than drifting toward an edit.

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
  - Present interface design as **code in a fenced code block** — read the
    shape as code, not as prose or bulleted markdown.

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

Treat the index as the user's to manage: **never stage or unstage on your own**. Both `git add` (moving work into the index) and `git reset` / `git restore --staged` (moving work back out of it) change what the user has committed there, so either one needs the user's explicit consent *before* you run it. If you believe a staging change is genuinely necessary mid-round, **ask first**; do not run it unprompted.

### Before committing

Before executing `git commit`, **show the proposed commit message and wait for confirmation**. Use Conventional Commits format (`feat:`, `fix:`, `docs:`, `chore:`, etc.).

## Agent skills

### Domain docs

Multi-context repo — `CONTEXT-MAP.md` at the root, one `CONTEXT.md` per context beside that context's code under `crates/`, and `docs/adr/` for system-wide decisions. See `docs/agents/domain.md`.
