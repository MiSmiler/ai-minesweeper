// The ai-player state machine (issue #119, #133): owns the Send run's phase, the
// AI Session's `sessionState` and message list, and the accumulated `reasoning`
// / `content` text. It is deliberately thin — phase + text accumulation + the
// Session's own data only. The discard confirm and the Load alerts live in the
// `app/` assembly layer.
//
// Generation tracking: each `begin()` / `end()` / `send()` bumps a
// generation counter, and the event callbacks capture the generation they were
// created in. A lifecycle change may happen while a previous SSE stream is
// still in flight (the frontend keeps the stream open on interrupt, #97), so
// events from a superseded generation are dropped and a stale stream can never
// corrupt the current state.
//
// The AI Session itself is the Agent's (ADR-0021): `begin()` asks the backend
// to begin one under an InputMode, `send()` appends to it, and `end()` drops
// it. The machine holds only the session's `unused` / `used` predicate.

import { isProviderError } from "./api";
import type {
  AiApi,
  InputMode,
  Message,
  ReplyEvent,
  SendFailure,
  ProviderError,
  SendRequest,
} from "./api";

/** The phase of the AiPlayer: the Send run's own phases (`idle` / `running` /
 * `done` / `interrupted`) plus `failed`, which any failure before or during a
 * run lands in — a Load failure, a refusal or a provider failure. */
export type AiPlayerPhase =
  "idle" | "running" | "done" | "interrupted" | "failed";

/** Whether an AI Session is live: `none` (no session), `unused` (created, no
 * Send yet), `used` (a Send has been made — the player's message is part of
 * the Session's history, whether or not the reply ever landed). The `used` half
 * drives the discard confirm and the refresh guard; the InputMode lock follows
 * the Session itself, not this predicate. */
export type SessionState = "none" | "unused" | "used";

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
  /** The Session's message list, oldest first; empty until the snapshot lands
   * (and if it never does). It belongs to the Session, not to a Send. */
  messages: Message[];
  /** Present for the image form: the screenshot the player sent (data URL). */
  userImageUrl?: string;
  /** Set only when `phase === "failed"`: why the Send produced no reply. */
  failure?: SendFailure;
}

export interface AiPlayerMachine {
  /** Loads the AI runtime and begins an AI Session on the backend under
   * `mode`. */
  begin(mode: InputMode): Promise<void>;
  /** Ends the live AI Session (New Game / PlayMode switch). */
  end(): void;
  /** Appends the current board to the live AI Session. */
  send(req: SendRequest): void;
  /** User-initiated cancel: POST /ai/interrupt (the SSE stays open). */
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
    messages: [],
  };
}

/** Builds an `AiPlayerMachine` over the given `AiApi`. The AiPlayer addresses no
 * Session by id (ADR-0021); the machine tracks only its `unused` / `used`
 * predicate and its own generation for superseded streams. */
export function createAiPlayerMachine(deps: { api: AiApi }): AiPlayerMachine {
  let state: AiPlayerState = idleState("none");
  let generation = 0;
  const listeners = new Set<(s: AiPlayerState) => void>();

  const emit = (): void => {
    const snapshot: AiPlayerState = { ...state };
    for (const cb of listeners) cb(snapshot);
  };

  /** Whether a Session is live: `begin` succeeded and no `end` has landed. */
  const hasLiveSession = (): boolean => state.sessionState !== "none";

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
        state = { ...state, phase: "done" };
        break;
      case "interrupted":
        // The caller's own act: the reply never lands.
        state = { ...state, phase: "interrupted" };
        break;
      case "provider_error":
        // A mid-stream provider failure: the same vocabulary as a refusal at
        // delivery, but the Provider's own cause.
        state = {
          ...state,
          phase: "failed",
          failure: { kind: "provider", error: e.error },
        };
        break;
    }
    emit();
  };

  const onFailure = (g: number, f: SendFailure): void => {
    if (g !== generation) return;
    state = { ...state, phase: "failed", failure: f };
    emit();
  };

  return {
    async begin(mode) {
      const g = ++generation;
      // The old session is being replaced: reset to `none` immediately so any
      // in-flight Send becomes stale.
      state = idleState("none");
      emit();
      try {
        await deps.api.begin(mode);
      } catch (err) {
        if (g !== generation) return; // superseded while beginning
        const providerError: ProviderError = isProviderError(err)
          ? err
          : {
              kind: "upstream",
              code: null,
              message: err instanceof Error ? err.message : String(err),
            };
        state = {
          ...state,
          phase: "failed",
          failure: { kind: "provider", error: providerError },
        };
        emit();
        return;
      }
      if (g !== generation) return; // superseded while beginning
      state = { ...state, sessionState: "unused" };
      emit();
      // The snapshot lands on top of a box that is already on screen; a failed
      // read leaves the list empty and never fails the Session.
      const messages = await deps.api.messages();
      if (g !== generation) return; // superseded while reading
      state = { ...state, messages };
      emit();
    },
    end() {
      generation++; // invalidate any in-flight stream
      state = idleState("none");
      emit();
    },
    send(req) {
      if (!hasLiveSession()) return; // no live session: a no-op
      const g = ++generation;
      state = {
        phase: "running",
        // The player sent it, whatever the reply turns out to be: the Session
        // is used from this moment. An Interrupt only cuts the assistant half.
        sessionState: "used",
        reasoning: "",
        content: "",
        user: "",
        userImageUrl: req.imageDataUrl,
        // The message list is the Session's: a Send clears this round, not it.
        messages: state.messages,
      };
      emit();
      deps.api.send(
        req,
        (e) => onEvent(g, e),
        (f) => onFailure(g, f),
      );
    },
    async interrupt_by_user() {
      if (!hasLiveSession()) return;
      await deps.api.interrupt_by_user();
    },
    onState(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
