# Issue tracker: Local + GitHub (dual-tracker)

Issues and specs for this repo live in **two trackers**: a Local Tracker (markdown files under `.scratch/`) and a GitHub Tracker (`MiSmiler/ai-minesweeper` issues). Before creating any ticket or spec, always confirm with the user which tracker to use — the **Default Tracker is Local**.

## Vocabulary

- **Tracker** — the place where Tickets live. This repo runs two: Local and GitHub.
- **Local Tracker** — markdown ticket files under `.scratch/<feature-slug>/tickets/`.
- **GitHub Tracker** — issues on `MiSmiler/ai-minesweeper`.
- **Ticket** — a unit of work published to a tracker; the canonical word for what `to-tickets` produces. Use it everywhere in our own artifacts.
- **Issue** — GitHub's platform term for a ticket. The `gh issue` CLI verb and GitHub's UI say "issue"; we don't fight that, but our own titles, bodies, and references say "ticket". No "Ticket: " title prefix.
- **Tracker Choice** — the confirmation step before creating tickets: ask "Local (default) or GitHub?".
- **Qualified reference** — cross-tracker references are `ticket:local:<feature-slug>/<NN>` and `ticket:github:<number>`. Local numbers restart at `01` per feature directory; GitHub numbers are repo-global, so a bare `#3` is ambiguous.

## Tracker Choice

- **When to ask**: before ANY creation-type publish — `/to-tickets` ticket batches and `/to-spec` specs. Triage and read operations never prompt; they follow the ticket's own tracker.
- **Default**: Local. Interactive sessions prompt "Local (default) / GitHub?" and accept Enter for Local; non-interactive / AFK sessions silently use Local.
- **Batch stickiness**: one `/to-tickets` run publishes its whole batch to one tracker. **Feature stickiness**: a feature's tickets stay in the tracker they started in; when confirming, note "this feature's existing tickets live in <tracker> — continue there?".
- **Dispatch**: publish mechanics live in the repo-local skills `.pi/skills/to-tickets-local/` and `.pi/skills/to-tickets-github/`. When a skill's generic instructions hardcode one tracker's shape (e.g. the global `to-tickets` step 5 writes to `.scratch/<feature-slug>/issues/`), follow THIS doc and the matching repo skill instead.

## Local Tracker conventions

- One feature per directory: `.scratch/<feature-slug>/`
- Tickets: one file per ticket at `.scratch/<feature-slug>/tickets/<NN>-<slug>.md`, numbered from `01` — never a single combined file
- Triage state is a `Status:` line near the top of each ticket file (`ready-for-agent` for new tickets)
- Comments and conversation history append to the bottom of the file under a `## Comments` heading

## GitHub Tracker conventions

- **Create a ticket**: `gh issue create --title "..." --body "..." --label ready-for-agent`. Use a heredoc for multi-line bodies.
- **Read a ticket**: `gh issue view <number> --comments`, filtering comments by `jq` and also fetching labels.
- **List tickets**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on a ticket**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repo from `git remote -v` — `gh` does this automatically when run inside a clone.

## Pull requests as a request surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`, PRs run through the same labels and states as issues, using the `gh pr` equivalents:

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>` for the diff.
- **List external PRs for triage**: `gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments` then keep only `authorAssociation` of `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, or `NONE` (drop `OWNER`/`MEMBER`/`COLLABORATOR`).
- **Comment / label / close**: `gh pr comment`, `gh pr edit --add-label`/`--remove-label`, `gh pr close`.

GitHub shares one number space across issues and PRs, so a bare `#42` may be either — resolve with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Confirm the Tracker Choice with the user (default Local), then use the matching repo skill:

- Local → `.pi/skills/to-tickets-local/`
- GitHub → `.pi/skills/to-tickets-github/`

## When a skill says "fetch the relevant ticket"

- **Local**: read the file at the referenced path (`.scratch/<feature-slug>/tickets/<NN>-<slug>.md`); the user normally passes the path or the qualified reference.
- **GitHub**: run `gh issue view <number> --comments`.

## Wayfinding operations

Used by `/wayfinder`. Read operations cover BOTH trackers — present local and GitHub tickets in separate buckets. The **map** and its **child** tickets live in the same tracker as the tickets they govern.

### Local Tracker

- **Map**: `.scratch/<effort>/map.md` — the Notes / Decisions-so-far / Fog body.
- **Child ticket**: `.scratch/<effort>/tickets/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`grilling`/`task`); a `Status:` line records `claimed`/`resolved`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `resolved`.
- **Frontier**: scan `.scratch/<effort>/tickets/` for files that are open, unblocked, and unclaimed; first by number wins.
- **Claim**: set `Status: claimed` and save before any work.
- **Resolve**: append the answer under an `## Answer` heading, set `Status: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.

### GitHub Tracker

- **Map**: a single issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label wayfinder:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (`gh api` on the sub-issues endpoint). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies** — the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only — the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: list the map's open children (`gh issue list --state open`, scoped to the map's sub-issues / task list), drop any with an open blocker (`issue_dependencies_summary.blocked_by > 0`, or an open issue in the `Blocked by` line) or an assignee; first in map order wins.
- **Claim**: `gh issue edit <n> --add-assignee @me` — the session's first write.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far.
