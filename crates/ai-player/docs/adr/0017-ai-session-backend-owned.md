# The AI Session is backend-owned and bound to one Game

> Superseded by ADR-0021: the Agent now owns the live Session — its id, its
> messages and the in-flight Send's cancel token. The policy below stands (one
> Session per Game, the InputMode bound at the first committed Turn); where the
> mechanism lives does not.

> Renamed by ADR-0019: `Guide` is now `AiPlayer`; the decision below is unchanged.

The advisor used to hold no conversation: `Guide::suggest` built a throwaway
`Session` per analysis, so history never accumulated and the `SessionStrategy`
dropdown named that fact as `per-analysis`. We replace it with one first-class
concept, the **AI Session**: a backend-owned conversation bound to one Game,
created explicitly, and spanning every Send of that Game. The backend owns it
because the DeepSeek `/chat/completions` API is stateless — the client must
resend the whole message list every turn, so someone must hold it, and the
board rendering already lives behind the adapter seam.

Considered options: a frontend-held transcript (rejected — resends every
analysis's payload, duplicates rendering, and moves model context into the UI);
a multi-session registry keyed by id (rejected — the server runs one Game at a
time, so a map buys nothing); implicit session-per-game creation (rejected —
the player asked for an explicit trigger).

An AI Session is empty until its first Turn commits; that same `empty` /
`non-empty` predicate drives both the InputMode lock and the discard confirm.
The InputMode is bound at the first committed Send rather than at creation, so
an empty session can still change its board legend. A Send commits the `user` +
`assistant` pair atomically and adds nothing on interrupt or a failed
Prepare, so the stored history never holds a dangling `user` turn.

Consequences: `Guide` becomes stateful and owns the live session plus its
in-flight cancel token, so the server's `ai_sessions` registry disappears; a
New Game ends the AI Session; context growth is unbounded (trim/summarize is
future work).
