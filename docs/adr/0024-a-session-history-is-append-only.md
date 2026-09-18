# A Session's history is append-only

> Amended by ADR-0026: an Interrupt still adds no Reply, but it does add an
> entry to the log — an Interrupt marker — where the reply would have gone.
>
> Amended by ADR-0027: the caller's messages land when the Send reaches the
> Provider; the reply lands on that Send's Run.

The Session used to commit a **Turn** — the `user` / `assistant` pair — as a unit: the caller's messages were held aside and written only when the stream reached `Done`, so an interrupted, failed or unread Send left no trace. That kept the history free of an unanswered `user` message, and it was paid for three times: one Send could produce exactly one Reply (ADR-0020), the InputMode could only bind once a Turn had committed (ADR-0017), and the AiPlayer grew a `prepare` step whose job was the checks the delayed write implied.

The premise does not hold. An Interrupt acts on the assistant's half — the reply being generated — not on the caller's message, which was sent. A history holding an unanswered `user` message is not corrupt: a model reads two consecutive `user` messages for what they are. This project is a small agent demo, not a framework, and the invariant insured against a case with no scenario behind it.

A Session's history is now append-only. `create_session` opens it with the caller's system prompt; a Send appends the caller's messages as soon as it reaches the Provider; the Run appends the reply when its stream reaches `Done`. An interrupted or failed Run leaves the caller's messages behind without a reply — an Interrupt appends a marker where the reply would have gone (ADR-0026).

Considered options: keeping the atomic write and renaming `Turn` (rejected — the mechanism was the cost, not the word); rolling the caller's messages back on interrupt by remembering the history length (rejected — the same deferral under another name, and it treats an Interrupt as an act on the caller's message).

Consequences: the InputMode becomes the Session's, fixed at `create_session` and changeable only by ending the Session, because the mode and the system prompt must agree for the Session's whole life. `Agent::send` takes no mode, `/ai/begin` carries one, and `ModeLock`, `ModeGuard` and `SendError::ModeMismatch` disappear with the lock they implemented. `Agent::has_committed_turn` disappears too: the system prompt is the history's first message, so no Send has to ask whether it is the first. The AiPlayer's `prepare` shrinks to the free `board_message` render, and the glossary loses **Turn** and **Prepare**. ADR-0020 is superseded (its argument was the Send/Turn 1:1), as is ADR-0017's InputMode binding. The frontend's session predicate is `none` / `unused` / `used`: `used` flips as soon as a Send is made, and the refresh guard follows it.
