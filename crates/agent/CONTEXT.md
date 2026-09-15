# agent

The runtime that runs an AI conversation: it owns the model and the Provider,
holds the messages, and answers Sends. It is deliberately ignorant of
Minesweeper — `game` is not one of its dependencies.

## Language

**Agent**:
The runtime that owns the model and the Provider and answers Sends. It is blind to what the messages mean.
_Avoid_: assistant, bot, AI

**Provider**:
The AI service that serves a model, plus the configuration needed to reach it: DeepSeek today, with the set open to others. Load resolves a Provider and validates its configuration and model.
_Avoid_: vendor, backend, endpoint

**Load**:
The Agent bringing its Provider and model up: the configuration is resolved and the model validated, before any Send. A Load failure leaves any live Session untouched.
_Avoid_: preflight, pre-flight, startup, initialization

**Session**:
The messages a caller has committed: the `user` / `assistant` Turns in order. A Session holds only committed Turns — an interrupted, failed, or unread stream adds none.
_Avoid_: conversation, chat, context, transcript

**Send**:
One call that appends a message to a Session and asks the Agent for a reply; the reply completes the Turn. A Send that fails before any content adds no Turn.
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

**InterruptReason**:
Why a stream stopped early: the caller cancelled it, or the Provider failed mid-stream (rate limit, timeout, upstream error).
_Avoid_: error kind, failure reason, stop reason
