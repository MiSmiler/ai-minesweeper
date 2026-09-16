# Reply is one assistant message

> Superseded by ADR-0024: a Session's history is append-only and the Turn is
> gone, so the 1:1 this ADR recorded — one Send, one `user` / `assistant` pair,
> one Reply — no longer follows from the commit rule.

Minting `Reply` next to `Turn` records today's relation: one Send commits one `user` / `assistant` pair, so one Send has exactly one Reply. The tool loop `AiPlay` will run (ADR-0013, ADR-0018: `GameHandle` / `tools()` have no caller yet) breaks that 1:1 — one Send will produce several Replies — while the commit stays where it is today: `Agent::stream` commits `pending` plus the assistant message per provider exchange, so a Send interrupted between exchanges leaves the earlier Replies in the Session. Whoever builds the tool loop must revisit **Turn** (it stops being one Send + one Reply), **Session** (an interrupted Send no longer adds nothing) and **Send** (failing between exchanges is a new case); `Reply` itself does not change.
