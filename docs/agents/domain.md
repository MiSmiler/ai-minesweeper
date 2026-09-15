# Domain Docs

How the workflow skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT-MAP.md`** at the repo root — it points at one `CONTEXT.md` per context, each living beside its code. Read each one relevant to the topic.
- **`docs/adr/`** — read ADRs that touch the area you're about to work in, and check `crates/<context>/docs/adr/` for context-scoped decisions.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The `domain-modeling` skill (reached via `grill-with-docs`) creates them lazily when terms or decisions actually get resolved.

## File structure

Single-context repo (most repos) — one glossary at the root:

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-event-sourced-orders.md
│   └── 0002-postgres-for-write-model.md
└── src/
```

This repo is multi-context (`CONTEXT-MAP.md` at the root). Its contexts do not all live under one stack directory — `game` and `ai-player` span Rust and TypeScript — so the map, not the tree, is the authority on where a context lives:

```
/
├── CONTEXT-MAP.md                     ← the contexts, their homes, the app-level terms
├── docs/adr/                          ← system-wide decisions
├── crates/
│   ├── game/     src/, CONTEXT.md, docs/adr/
│   ├── agent/    src/, CONTEXT.md
│   └── ai-player/src/, prompts/, CONTEXT.md, docs/adr/
├── src/                               ← the app (root package): binary + transport
└── frontend/src/                      ← game/, ai-player/, app/, infra/
```

## Use the glossary's vocabulary

When your output names a domain concept — in a spec, a seam, an interface, a ticket title, a test name — use the term as `CONTEXT.md` defines it. Drift to a synonym the glossary explicitly avoids is a finding.

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (event-sourced orders) — but worth reopening because…_
