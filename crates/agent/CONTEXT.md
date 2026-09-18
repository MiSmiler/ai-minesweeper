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
The log its Sends have built — the system prompt it was created with, then every entry the caller's Sends have landed, in order — plus the Run in flight against it. `create_session` opens the log with the caller's system prompt; a Send's messages land when the Send reaches the Provider, and its Run lands the reply on `Done`. An Interrupt appends a marker in place of that reply, so the log can hold entries that are not Replies; what the Provider is sent is a lossy projection of it. At most one is live per Agent, so a caller addresses the Agent and never a Session.
_Avoid_: conversation, chat, context, transcript

**Send**:
One call that appends the caller's messages to a Session and starts a Run against it. It ends when the messages land: reaching the Provider is what ends a Send, and everything after that — the stream, the reply, the Interrupt — is the Run's. A Send that never reaches the Provider lands nothing, and its failure is the Send's, because there was no Run to have failed.
_Avoid_: analysis, request, prompt, query

**Run**:
The span from a Send's messages landing to the stream ending: the deltas stream on it, the reply lands on `Done`, and the caller's Interrupt stops it. A Run that never reaches its end leaves the messages behind with no reply; an interrupted one lands an Interrupt marker in its place. What it reports is its own — a mid-stream Provider failure, or the caller's Interrupt.
_Avoid_: execution, task, job, exchange

**Reply**:
The assistant's half of a Run — the Agent's answer to the caller. Not every assistant-shaped entry is one: the Interrupt marker records an act, not an answer.
_Avoid_: response, answer, completion, message

**Interrupt**:
The caller's act of stopping the live Run before it ends: the reply never lands. It is an act, not a cause — a Provider failing mid-stream is not an Interrupt.
_Avoid_: cancel, abort, stop
