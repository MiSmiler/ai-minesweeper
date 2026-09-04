import { log } from "../infra/log";

// Frontend wire type contract for the AI guide transport (issue #114) and the
// real SSE consumer (issue #119).
//
// The backend wire events are isomorphic to the server's `GuideEventDto`
// ([src/server/ai_routes.rs](/src/server/ai_routes.rs)). `startGuide` POSTs
// `/ai/guide/:id` and parses the SSE stream: each `data:` payload is either
// `[DONE]` (synthesized locally as `{kind:"sse_done"}` — the backend
// `GuideEventDto` has no `Done` variant, a finished stream just ends as
// `data: [DONE]`) or a `GuideEventDto` JSON. The backend domain event is the
// shared `StreamChunk` (`Ok(StreamChunk)` = delta / Done; a mid-stream break
// is `Err(InterruptReason)`).

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

/** The AI slice entry point, injected via `AppDeps`. The real implementation
 * (`createAiApi`) consumes the backend `/ai/guide` SSE transport (issue #119).
 */
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

/** Builds the real `AiApi` that talks to the backend `/ai/guide` SSE routes.
 * `startGuide` does not `abort` the SSE on interrupt — the backend emits the
 * `interrupt` event on the open stream (issue #97, #119). */
export function createAiApi(): AiApi {
  return {
    startGuide(sessionId, req, onEvent, onProviderError) {
      void consumeGuide(sessionId, req, onEvent, onProviderError);
    },
    async interrupt_by_user(sessionId) {
      try {
        const res = await fetch(`/ai/guide/${sessionId}/interrupt`, {
          method: "POST",
        });
        if (!res.ok) {
          log.error(
            `POST /ai/guide/${sessionId}/interrupt failed: ${res.status}`,
          );
        }
        return res;
      } catch (err) {
        log.error("POST /ai/guide/:id/interrupt failed", err);
        return null;
      }
    },
  };
}

/** The frontend request body on the wire. The frontend type keeps the
 * camelCase `imageDataUrl` (issue #114), but the backend
 * `ai_adapter::GuideRequest` field is snake_case `image_data_url`. */
function wireRequest(req: GuideRequest): Record<string, unknown> {
  return { format: req.format, image_data_url: req.imageDataUrl };
}

/** POSTs the guide request and forwards the SSE stream to `onEvent`. */
async function consumeGuide(
  sessionId: string,
  req: GuideRequest,
  onEvent: (e: GuideEvent) => void,
  onProviderError: (e: ProviderError) => void,
): Promise<void> {
  let res: Response;
  try {
    res = await fetch(`/ai/guide/${encodeURIComponent(sessionId)}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(wireRequest(req)),
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    onProviderError({ kind: "upstream", code: null, message });
    return;
  }
  if (!res.ok) {
    onProviderError(await readProviderError(res));
    return;
  }
  await consumeSse(res, onEvent);
}

/** Parses a non-OK guide response into a `ProviderError` (or a fallback). */
async function readProviderError(res: Response): Promise<ProviderError> {
  try {
    const body: unknown = await res.json();
    if (body && typeof body === "object" && "kind" in body) {
      return body as ProviderError;
    }
  } catch {
    // Not JSON: fall through to the status-based fallback.
  }
  return {
    kind: "upstream",
    code: res.status,
    message: `AI guide request failed (HTTP ${res.status})`,
  };
}

/** Reads the response body as an SSE stream and emits `GuideEvent`s. */
async function consumeSse(
  res: Response,
  onEvent: (e: GuideEvent) => void,
): Promise<void> {
  if (!res.body) return;
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    // Normalise line endings; the SSE spec allows \r\n / \r / \n.
    buffer = buffer.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
    let sep: number;
    while ((sep = buffer.indexOf("\n\n")) !== -1) {
      const block = buffer.slice(0, sep);
      buffer = buffer.slice(sep + 2);
      const data = parseDataField(block);
      if (data === null) continue;
      if (data === "[DONE]") {
        onEvent({ kind: "sse_done" });
        return; // A finished stream ends here; stop reading.
      }
      try {
        onEvent(JSON.parse(data) as GuideEvent);
      } catch (err) {
        log.error(`Failed to parse SSE event: ${data}`, err);
      }
    }
  }
}

/** Extracts the concatenated `data:` payload of one SSE block; `null` if the
 * block carries only metadata (event/id/comment) or an empty `data:`. */
function parseDataField(block: string): string | null {
  let data = "";
  for (const line of block.split("\n")) {
    if (line.startsWith("data:")) {
      data += line.slice(5).replace(/^ /, "") + "\n";
    }
  }
  data = data.replace(/\n$/, ""); // drop the trailing join newline
  return data === "" ? null : data;
}
