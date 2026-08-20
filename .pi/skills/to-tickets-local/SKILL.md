---
name: to-tickets-local
description: Publish tracer-bullet tickets to this repo's Local Tracker — markdown files under .scratch/<feature>/tickets/. Invoke directly, or via /to-tickets / /to-spec when the user picks Local (the default).
disable-model-invocation: true
---

# To Tickets — Local Tracker

Publish approved tickets as markdown files in the Local Tracker. This skill covers ONLY the Local Tracker mechanics; the ticket breakdown (gathering context, slicing, quizzing the user) comes from the global `/to-tickets` skill.

## When to use

Two entry paths (see `docs/agents/issue-tracker.md`):

- **Direct**: the user invokes `/to-tickets-local` — publish to the Local Tracker, no confirmation.
- **Dispatched**: `/to-tickets` or `/to-spec` runs the full workflow and at publish the user picks Local (the default) — then follow this skill for the publish mechanics.

Read operations and wayfinding live in `docs/agents/issue-tracker.md`, not here.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/tickets/` (note `tickets/`, not `issues/`).
- One file per ticket: `.scratch/<feature-slug>/tickets/<NN>-<slug>.md`, numbered from `01` in dependency order (blockers first). Never a single combined file.
- Call them **tickets**, never issues.
- Reference a ticket within its feature as `ticket:<NN>`; across trackers use the qualified form `ticket:local:<feature-slug>/<NN>`.

## Process

1. Create the feature directory if needed: `.scratch/<feature-slug>/tickets/`.
2. Write one file per ticket in dependency order, using the template below.
3. Fill `Blocked by:` with the numbers/titles of the tickets that gate this one, or "None — can start immediately".
4. Set `Status: ready-for-agent` on every ticket.
5. Append comments and conversation history to the bottom of the file under a `## Comments` heading as they happen.

## Template

# <NN> — <Ticket title>

**What to build:** the end-to-end behaviour this ticket makes work, from the user's perspective — not a layer-by-layer implementation list.

**Blocked by:** the numbers/titles of the tickets that gate this one, or "None — can start immediately".

**Status:** ready-for-agent

- [ ] Acceptance criterion 1
- [ ] Acceptance criterion 2
