---
name: code-review-for-naming
description: Reviews identifier naming across a codebase — types, functions, fields, locals, and naming-related API design — reporting findings by severity without touching the code.
disable-model-invocation: true
---

# Review Naming

Audit identifier naming in the target source files and report findings. The deliverable is a report, never edits — present findings and wait for the user's confirmation before modifying anything.

## Scope

- Default: all source files. Follow the user's call if they narrow the scope (a file, a module, a diff).
- Judge each language by its own conventions. In mixed-language projects (e.g. a Tauri app: web + Rust), judge each part separately.

## Process

1. Read the target files in full.
2. Read the domain docs if present: `CONTEXT.md` (or `CONTEXT-MAP.md`) at the repo root, plus `docs/adr/`. If none exist, skip this step silently.
3. Review naming along these directions:
   - Does each identifier say what it holds or does — types, functions, fields, locals, parameters, test names? A name that takes effort to decode is a finding.
   - Does it follow the language's conventions (case styles, getter naming, boolean prefixes)? Judge per language on the spot — no fixed checklist.
   - Naming-adjacent API design: does the name match what it returns, the order of its parameters, and the state it stores? Flag mismatches that mislead callers (reversed tuple orders, duplicated state under two names, a name that promises less than the type delivers).
   - If domain docs exist: do identifiers use the glossary's vocabulary? Flag conflicts with defined terms and avoid-lists, plus gaps or inaccuracies in the docs themselves.
4. Report, grouped by severity: domain-language conflicts first, then name/behavior mismatches, then naming-adjacent API design, then minor issues. Each finding: file:line, current name, what's wrong, suggested rename. If nothing stands out, say so plainly — an honest clean bill beats padding.
