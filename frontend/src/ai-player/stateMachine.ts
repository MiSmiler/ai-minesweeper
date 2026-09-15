// The ai-player state machine (issue #119, #133): owns the Send run's phase, the
// AI Session's `sessionState`, and the accumulated `reasoning` / `content`
// text. It is deliberately thin — phase + text accumulation + session
// lifecycle only. The discard confirm and the Load / Prepare alerts live in the
// `app/` assembly layer.
//
// Generation tracking: each `newSession()` / `endSession()` / `send()` bumps a
// generation counter, and the event callbacks capture the generation they were
// created in. A lifecycle change may happen while a previous SSE stream is
// still in flight (the frontend keeps the stream open on interrupt, #97), so
// events from a superseded generation are dropped and a stale stream can never
// corrupt the current state.
//
// The AI Session itself is backend-owned (ADR-0017): `newSession()` asks the
// backend for an id, `send()` appends to it, and `endSession()` drops it. The
// machine holds only the id and the session's `empty` / `non-empty` predicate.

import { isProviderError } from "./api";
import type {
  AiApi,
  ReplyEvent,
  InterruptReason,
  ProviderError,
  SendRequest,
} from "./api";

/** The phase of the AiPlayer: the Send run's own phases (`idle` / `running` /
 * `done` / `interrupted`) plus the two failures before any content —
 * `load-failed` (session creation) and `prepare-failed` (a Send). */
export type AiPlayerPhase =
  | "idle"
  | "running"
  | "done"
  | "interrupted"
  | "load-failed"
  | "prepare-failed";

/** Whether an AI Session is live: `none` (no session), `empty` (created, no
 * committed Turn), `non-empty` (at least one committed Turn). The `empty` /
 * `non-empty` split drives both the InputMode lock and the discard confirm. */
export type SessionState = "none" | "empty" | "non-empty";

/** The accumulated state of the current Send and its AI Session. */
export interface AiPlayerState {
  phase: AiPlayerPhase;
  sessionState: SessionState;
  /** Accumulated reasoning stream (light, collapsible in the box). */
  reasoning: string;
  /** Accumulated content stream (normal font, not collapsible). */
  content: string;
  /** The verbatim player message (the `role: user` turn), echoed by the backend. */
  user: string;
  /** Present for the image form: the screenshot the player sent (data URL). */
  userImageUrl?: string;
  /** Set only when `phase === "interrupted"`. */
  interruptReason?: InterruptReason;
  /** Set only when `phase` is `load-failed` or `prepare-failed`. */
  providerError?: ProviderError;
}

export interface AiPlayerMachine {
  /** Loads the AI runtime and requests an empty AI Session from the backend. */
  newSession(): Promise<void>;
  /** Ends the live AI Session (New Game / PlayMode switch). */
  endSession(): void;
  /** Appends the current board to the live AI Session. */
  send(req: SendRequest): void;
  /** User-initiated cancel: POST /ai/session/:id/interrupt (the SSE stays open). */
  interrupt_by_user(): Promise<void>;
  /** Subscribes to state changes; returns an unsubscribe. */
  onState(cb: (state: AiPlayerState) => void): () => void;
}

/** An idle state with the given session state and no accumulated text. */
function idleState(sessionState: SessionState): AiPlayerState {
  return {
    phase: "idle",
    sessionState,
    reasoning: "",
    content: "",
    user: "",
  };
}

/** Builds an `AiPlayerMachine` over the given `AiApi`. The session id is issued by
 * the backend (`createSession`); the machine holds it for `send` / `interrupt`. */
export function createAiPlayerMachine(deps: { api: AiApi }): AiPlayerMachine {
  let state: AiPlayerState = idleState("none");
  let generation = 0;
  let sessionId: string | null = null;
  const listeners = new Set<(s: AiPlayerState) => void>();

  const emit = (): void => {
    const snapshot: AiPlayerState = { ...state };
    for (const cb of listeners) cb(snapshot);
  };

  const onEvent = (g: number, e: ReplyEvent): void => {
    if (g !== generation) return; // a stale stream from a superseded run
    switch (e.kind) {
      case "reasoning":
        state = { ...state, reasoning: state.reasoning + e.text };
        break;
      case "content":
        state = { ...state, content: state.content + e.text };
        break;
      case "user":
        state = { ...state, user: e.text };
        break;
      case "sse_done":
        // A committed Turn: an empty session becomes non-empty.
        state = { ...state, phase: "done", sessionState: "non-empty" };
        break;
      case "interrupt":
        state = { ...state, phase: "interrupted", interruptReason: e.reason };
        break;
    }
    emit();
  };

  const onProviderError = (g: number, e: ProviderError): void => {
    if (g !== generation) return;
    state = { ...state, phase: "prepare-failed", providerError: e };
    emit();
  };

  return {
    async newSession() {
      const g = ++generation;
      // The old session is being replaced: drop its id immediately so any
      // in-flight Send becomes stale.
      sessionId = null;
      state = idleState("none");
      emit();
      let created: { sessionId: string };
      try {
        created = await deps.api.createSession();
      } catch (err) {
        if (g !== generation) return; // superseded while creating
        const providerError: ProviderError = isProviderError(err)
          ? err
          : {
              kind: "upstream",
              code: null,
              message: err instanceof Error ? err.message : String(err),
            };
        state = { ...state, phase: "load-failed", providerError };
        emit();
        return;
      }
      if (g !== generation) return; // superseded while creating
      sessionId = created.sessionId;
      state = { ...state, sessionState: "empty" };
      emit();
    },
    endSession() {
      generation++; // invalidate any in-flight stream
      sessionId = null;
      state = idleState("none");
      emit();
    },
    send(req) {
      if (sessionId === null) return; // no live session: a no-op
      const g = ++generation;
      state = {
        phase: "running",
        sessionState: state.sessionState,
        reasoning: "",
        content: "",
        user: "",
        userImageUrl: req.imageDataUrl,
      };
      emit();
      deps.api.send(
        sessionId,
        req,
        (e) => onEvent(g, e),
        (e) => onProviderError(g, e),
      );
    },
    async interrupt_by_user() {
      if (sessionId === null) return;
      await deps.api.interrupt_by_user(sessionId);
    },
    onState(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
