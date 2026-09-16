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
The history its Sends have built — the system prompt it was created with, then every message the caller's Sends have landed, in order — plus the Send in flight against it. `create_session` opens the history with the caller's system prompt; a Send appends the caller's messages as soon as it reaches the Provider, and the reply follows on `Done`, so an interrupted or failed Send leaves the caller's messages behind with no reply. At most one is live per Agent, so a caller addresses the Agent and never a Session.
_Avoid_: conversation, chat, context, transcript

**Send**:
One call that appends the caller's messages to a Session and asks the Agent for a reply. The messages land when the Send reaches the Provider; the reply lands on `Done`. A Send that never reaches its end — interrupted, failed or unread — leaves the messages behind with no reply.
_Avoid_: analysis, request, prompt, query

**Reply**:
The assistant's half of a Send — the Agent's answer to the caller.
_Avoid_: response, answer, completion, message

**Interrupt**:
The caller's act of stopping the live Send before it ends: the reply never lands. It is an act, not a cause — a Provider failing mid-stream is not an Interrupt.
_Avoid_: cancel, abort, stop
