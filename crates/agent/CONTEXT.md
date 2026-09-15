# agent

The runtime behind a Session: it owns the model, the Provider and the live
Session, and answers Sends. It is deliberately ignorant of Minesweeper —
`game` is not one of its dependencies.

## Language

**Agent**:
The runtime that owns the model, the Provider and the live Session, and answers Sends. It is blind to what the messages mean.
_Avoid_: assistant, bot, AI

**Provider**:
The AI service that serves a model, plus the configuration needed to reach it: DeepSeek today, with the set open to others. Load resolves a Provider and validates its configuration and model.
_Avoid_: vendor, backend, endpoint

**Load**:
The Agent bringing its Provider and model up: the configuration is resolved and the model validated, before any Send. A Load failure leaves any live Session untouched.
_Avoid_: preflight, pre-flight, startup, initialization

**Session**:
The Turns a caller has committed — the `user` / `assistant` pairs in order — plus the Send in flight against them; an interrupted, failed or unread stream adds no Turn. It carries an id and is created and ended by the Agent, and at most one is live per Agent, so a caller addresses the Agent and never a Session.
_Avoid_: conversation, chat, context, transcript

**Send**:
One call that appends a message to a Session and asks the Agent for a reply; the reply completes the Turn. A Send that does not reach its end — interrupted, failed or unread — adds no Turn.
_Avoid_: analysis, request, prompt, query

**Prepare**:
A Send's pre-processing — the payload the caller supplies and the checks in front of it — before any reply content arrives. A failed Prepare adds no Turn and leaves the Session live.
_Avoid_: preflight, pre-flight, preamble

**Turn**:
One Send together with the reply: the committed `user` / `assistant` pair. A Turn enters a Session as a unit.
_Avoid_: round, exchange, message pair

**Reply**:
The assistant's half of a Turn — the Agent's answer to a Send.
_Avoid_: response, answer, completion, message

**Interrupt**:
The caller's act of stopping the live Send before it ends; the Send commits no Turn. It is an act, not a cause — a Provider failing mid-stream is not an Interrupt.
_Avoid_: cancel, abort, stop
