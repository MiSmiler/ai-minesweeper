---
name: to-tickets-github
description: Publish tracer-bullet tickets to this repo's GitHub Tracker (MiSmiler/ai-minesweeper) with the gh CLI. Use when /to-tickets or /to-spec chooses the GitHub Tracker.
---

# To Tickets — GitHub Tracker

Publish approved tickets as GitHub issues. This skill covers ONLY the GitHub Tracker mechanics; the ticket breakdown (gathering context, slicing, quizzing the user) comes from the global `/to-tickets` skill.

## When to use

Use this skill when the Tracker Choice (see `docs/agents/issue-tracker.md`) selects the GitHub Tracker. The dispatch happens inside `/to-tickets` step 5: confirm the tracker with the user first, then follow the matching skill. Read operations, wayfinding, and native-blocking mechanics live in `docs/agents/issue-tracker.md`, not here.

## Conventions

- Each ticket is a GitHub issue on `MiSmiler/ai-minesweeper` — infer the repo from `git remote -v`; `gh` does this automatically when run inside a clone.
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
