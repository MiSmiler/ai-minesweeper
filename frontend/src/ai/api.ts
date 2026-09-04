// Frontend wire type contract for the AI guide transport (issue #114).
//
// The backend wire events are isomorphic to the server's `GuideEventDto`
// ([src/server/ai_routes.rs](/src/server/ai_routes.rs)). Consuming the real
// SSE stream is out of scope for this ticket: these types are the seam the
// `app/` composition and the stub analyzer use. `sse_done` is synthesized by
// the frontend when it reads the SSE `[DONE]` terminator; the wire never
// produced it (the backend `GuideEventDto` has no `Done` variant — a finished
// stream just ends as `data: [DONE]`). The backend domain event is the shared
// `StreamChunk` (`Ok(StreamChunk)` = delta / Done; a mid-stream break is
// `Err(InterruptReason)`), which the frontend translates into these wire
// events.

/** The #94 presentation forms of a board, mirrored from the backend kebab-case
 * (`ai_adapter::BoardFormat`). "model is a provider-specific name string, not
 * a literal union" — the model is never sent by the frontend. */
export type BoardFormat =
  "simple-text" | "emoji" | "full-coordinates" | "image";

/** The termination reason (#97); the backend decides the final state. */
export type InterruptReason =
  "user_interrupt" | "rate_limit" | "timeout" | "upstream_error" | "unknown";

/** A frontend-consumed wire event. `sse_done` is synthesized locally when the
 * SSE `[DONE]` is read; the wire never emits it. */
export type GuideEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "sse_done" }
  | { kind: "interrupt"; reason: InterruptReason };

/** = backend `ai::protocol::ProviderError`. */
export type ProviderError = {
  kind: "config" | "upstream";
  code: number | null;
  message: string;
};

/** The frontend's request: only `format` plus an optional `imageDataUrl` for
 * the image form. No model is sent — the backend picks its DeepSeek default. */
export interface GuideRequest {
  format: BoardFormat;
  imageDataUrl?: string;
}

/** The AI slice entry point, injected via `AppDeps`. This ticket uses a
 * stub/mock; the real SSE consumer lands in a later ticket. */
export interface AiApi {
  /** Starts one analysis (the "Analyze" button): streams `GuideEvent`s as they arrive. */
  startGuide(
    sessionId: string,
    req: GuideRequest,
    onEvent: (e: GuideEvent) => void,
    onProviderError: (e: ProviderError) => void,
  ): void;
  /** Cancels the running analysis for a session (the "Interrupt" button). */
  interrupt_by_user(sessionId: string): Promise<unknown>;
}
