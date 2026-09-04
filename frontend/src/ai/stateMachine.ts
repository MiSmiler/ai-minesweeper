// The guide state machine (issue #119): owns the analysis run's phase and the
// accumulated `reasoning` / `content` text. It is deliberately thin — the
// phase + text accumulation only. History binding, format-change confirm, and
// the pre-flight alert all live in the `app/` assembly layer.
//
// Generation tracking: each `start()` / `reset()` bumps a generation counter.
// `reset()` (new game, format change) or a fresh `start()` may happen while a
// previous SSE stream is still in flight (the frontend keeps the stream open on
// interrupt, #97). Events from a superseded generation are dropped so a stale
// stream can never corrupt the current state.

import type {
  AiApi,
  GuideEvent,
  GuideRequest,
  InterruptReason,
  ProviderError,
} from "./api";

/** The phase of a single guide analysis run. */
export type GuidePhase =
  "idle" | "running" | "done" | "interrupted" | "preflight-failed";

/** The accumulated state of the current analysis. `reasoning` and `content`
 * are the two streams, accumulated across `reasoning` / `content` events. */
export interface GuideState {
  phase: GuidePhase;
  /** Accumulated reasoning stream (light, collapsible in the dialog). */
  reasoning: string;
  /** Accumulated content stream (normal font, not collapsible). */
  content: string;
  /** Set only when `phase === "interrupted"`. */
  interruptReason?: InterruptReason;
  /** Set only when `phase === "preflight-failed"`. */
  providerError?: ProviderError;
}

export interface GuideMachine {
  /** Starts one round of analysis; `GuideMachine.start()` 一次 = 一次 `Guide::suggest`. */
  start(req: GuideRequest): void;
  /** User-initiated cancel: POST /ai/guide/:id/interrupt (the SSE stays open). */
  interrupt_by_user(): Promise<void>;
  /** Clears the run (input-format change / new game / mode switch). */
  reset(): void;
  /** Subscribes to state changes; returns an unsubscribe. */
  onState(cb: (state: GuideState) => void): () => void;
}

/** Builds a `GuideMachine` over the given `AiApi`. `newSessionId` is called
 * once per `start()` so each analysis is independently cancelable. */
export function createGuideMachine(deps: {
  api: AiApi;
  newSessionId: () => string;
}): GuideMachine {
  let state: GuideState = { phase: "idle", reasoning: "", content: "" };
  let generation = 0;
  let sessionId = deps.newSessionId();
  const listeners = new Set<(s: GuideState) => void>();

  const emit = (): void => {
    const snapshot: GuideState = { ...state };
    for (const cb of listeners) cb(snapshot);
  };

  const onEvent = (g: number, e: GuideEvent): void => {
    if (g !== generation) return; // a stale stream from a superseded run
    switch (e.kind) {
      case "reasoning":
        state = { ...state, reasoning: state.reasoning + e.text };
        break;
      case "content":
        state = { ...state, content: state.content + e.text };
        break;
      case "sse_done":
        state = { ...state, phase: "done" };
        break;
      case "interrupt":
        state = { ...state, phase: "interrupted", interruptReason: e.reason };
        break;
    }
    emit();
  };

  const onProviderError = (g: number, e: ProviderError): void => {
    if (g !== generation) return;
    state = { ...state, phase: "preflight-failed", providerError: e };
    emit();
  };

  return {
    start(req) {
      const g = ++generation;
      sessionId = deps.newSessionId();
      state = { phase: "running", reasoning: "", content: "" };
      emit();
      deps.api.startGuide(
        sessionId,
        req,
        (e) => onEvent(g, e),
        (e) => onProviderError(g, e),
      );
    },
    async interrupt_by_user() {
      await deps.api.interrupt_by_user(sessionId);
    },
    reset() {
      generation++; // invalidate any in-flight stream
      state = { phase: "idle", reasoning: "", content: "" };
      emit();
    },
    onState(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
