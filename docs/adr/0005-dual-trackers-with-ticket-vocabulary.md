# Dual trackers with ticket-first vocabulary

The repo tracks work in two issue trackers at once — a Local Tracker (markdown files under `.scratch/<feature>/tickets/`) and the GitHub Tracker (`MiSmiler/ai-minesweeper`) — instead of the single tracker the engineering skills assume. Before creating any ticket or spec, the agent confirms the Tracker Choice with the user; Local is the default, one run's whole batch goes to one tracker, and a feature's subsequent batches stay in the tracker it started in. Created tickets are called **tickets**, never issues, and local tickets live under `.scratch/<feature>/tickets/` rather than the skills' hardcoded `issues/`.

The mechanics live in two repo-local skills (`.pi/skills/to-tickets-local/` and `.pi/skills/to-tickets-github/`) plus prose in `docs/agents/issue-tracker.md`, rather than changing the global skills in `~/.agents/skills/` — those are shared across repos and stay single-tracker.

## Considered Options

- **Single tracker (GitHub or Local only)** — the skills' designed shape; rejected because local tickets (`.scratch/`) and GitHub issues (this repo's #33 and friends) were both already in use, and the user wanted an explicit choice at creation time with Local as the default.
- **Repo-local config prose only** — viable (only one hardcoded `issues/` path in the global `to-tickets` to override), but the mechanics would live buried in prose with lower discoverability than named skills.
- **Change the global skills to native multi-tracker** — rejected: blast radius across every repo using `~/.agents/skills/`, all of which are single-tracker.

## Consequences

- The repo-local skills duplicate a little template content from the global `to-tickets` and must be kept in sync manually if the global templates change.
- GitHub's UI and the `gh issue` CLI verb still say "issue" — platform vocabulary we don't fight; GitHub titles get no "Ticket: " prefix.
- Local tickets are per-directory and gitignored (`.scratch/`); GitHub tickets are shared across this repo's worktrees.
