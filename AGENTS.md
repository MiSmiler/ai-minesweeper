## Development habits

### Before modifying code

When investigating a problem (debugging, diagnosing an error, exploring the codebase), **report findings first and wait for confirmation** before making any changes. Do not jump from investigation directly into editing.

### After a coding round

A round of code changes (feature, fix, or a batch of edits) is not done until formatting and tests pass:

1. Run `cargo fmt` to format the code.
2. Run `cargo test` and ensure everything passes.
3. If a failure is clearly caused by your changes, fix it before reporting done. If the cause is unclear or unrelated, report it and wait for confirmation first.
4. Skip steps 1-2 when the round touched only non-code files (docs, markdown, etc.).

### Before committing

Before executing `git commit`, **show the proposed commit message and wait for confirmation**. Use Conventional Commits format (`feat:`, `fix:`, `docs:`, `chore:`, etc.).

## Agent skills

### Issue tracker

Issues are tracked as local markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage labels use the default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context repo — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
