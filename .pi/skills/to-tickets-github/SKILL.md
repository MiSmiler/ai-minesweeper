---
name: to-tickets-github
description: Publish tracer-bullet tickets to this repo's GitHub Tracker — the current repo's issues — with the gh CLI. Invoke directly, or via /to-tickets / /to-spec when the user picks GitHub.
disable-model-invocation: true
---

# To Tickets — GitHub Tracker

Publish approved tickets as GitHub issues. This skill covers the GitHub Tracker mechanics; the ticket breakdown (gathering context, slicing, quizzing the user) follows the global `/to-tickets` skill.

## When to use

Two entry paths (see `docs/agents/issue-tracker.md`):

- **Direct**: the user invokes `/to-tickets-github` — publish to the GitHub Tracker. The invocation commits the tracker choice only, never the tickets.
- **Dispatched**: `/to-tickets` or `/to-spec` runs the full workflow and at publish the user picks GitHub — then follow this skill for the publish mechanics.

Read operations, wayfinding, and native-blocking mechanics live in `docs/agents/issue-tracker.md`, not here.

## Publish gate

This skill never invents tickets — it publishes ones the user has approved:

- **Dispatched path**: the breakdown was just approved via `/to-tickets` / `/to-spec`; publish it as-is.
- **Direct path**: if the conversation already holds an approved breakdown (the user explicitly approved a proposal), publish it as-is. Otherwise run the breakdown first — gather context, draft the vertical slices, and present them to the user for approval, exactly as the global `/to-tickets` skill does before publishing (its steps 1–4) — then publish only after the user approves. Never publish tickets the user has not seen and approved.

## Conventions

- Each ticket is a GitHub issue on the current repo — `gh` resolves the repo automatically inside a clone (`gh repo view --json nameWithOwner --jq .nameWithOwner`); never hardcode a repo name.
- Call them **tickets** in every artifact we write (titles, bodies, comments, references). GitHub's own UI and the `gh issue` CLI verb still say "issue" — platform vocabulary we don't fight.
- Do NOT prefix titles with "Ticket: " — keep titles clean for the issue list.
- Reference a ticket as `ticket:github:<number>` in cross-tracker contexts; within GitHub, the bare `#<number>` is fine.
- Every ticket gets the `ready-for-agent` triage label (strings per `docs/agents/triage-labels.md`).

## Process

1. Publish one issue per ticket in dependency order (blockers first) so each blocking edge can reference real identifiers.
2. `gh issue create --title "<title>" --body "<body>" --label ready-for-agent` — use a heredoc for multi-line bodies.
3. Record each created issue's number and reference it in later tickets' `Blocked by:`.
4. Blocking edges: prefer GitHub's native dependencies (see the Wayfinding operations in `docs/agents/issue-tracker.md`); where unavailable, set each ticket's `Blocked by:` to the blocking issues' numbers.
5. Do NOT close or modify any parent issue.

## Template

## Parent

A reference to the parent issue on the tracker (if the source was an existing issue, otherwise omit this section).

## What to build

The end-to-end behaviour this ticket makes work, from the user's perspective — not layer-by-layer implementation.

## Acceptance criteria

- [ ] Criterion 1
- [ ] Criterion 2

## Blocked by

- A reference to each blocking ticket, or "None — can start immediately".
