import { log } from "../infra/log";

// Frontend wire type contract for the AI Session transport (issue #114, #131)
// and the real SSE consumer (issue #119).
//
// The backend wire events are isomorphic to the server's `ReplyEvent`
// ([src/server/ai_routes.rs](/src/server/ai_routes.rs)). `send` POSTs
// `/ai/send` and parses the SSE stream: each `data:` payload is either
// `[DONE]` (synthesized locally as `{kind:"sse_done"}` — the backend
// `ReplyEvent` has no `Done` variant, a finished stream just ends as
// `data: [DONE]`) or a `ReplyEvent` JSON. A mid-stream break is one of the two
// explicit events: `interrupted` (the caller's act) or `provider_error` (the
// Provider's failure, carrying its own cause).

/** The input modes, mirrored from the backend kebab-case
 * (`ai_adapter::InputMode`). "model is a provider-specific name string, not
 * a literal union" — the model is never sent by the frontend. */
export type InputMode = "plain" | "emoji" | "image";

/** The #122 reasoning-depth control: `off` disables thinking mode; the rest
 * set the `reasoning_effort`. Mirrored from `ai_adapter::ThinkingLevel`. */
export type ThinkingLevel = "off" | "low" | "high" | "max";

/** A frontend-consumed wire event. `sse_done` is synthesized locally when the
 * SSE `[DONE]` is read; the wire never emits it. `user` is the backend echo of
 * the player's message (issue #124), emitted first on the stream.
 * `interrupted` is the caller's own Interrupt; `provider_error` carries the
 * Provider's own failure. */
export type ReplyEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "user"; text: string }
  | { kind: "sse_done" }
  | { kind: "interrupted" }
  | { kind: "provider_error"; error: ProviderError };

/** = backend `ai::protocol::ProviderError`. */
export type ProviderError = {
  kind: "config" | "upstream";
  code: number | null;
  message: string;
};

/** Why a Send produced no reply, as the frontend sees it. A `provider` failure
 * is the Provider's own cause, wherever it happened (Load, or a Send) and
 * carries it intact; a `refused` failure is the AiPlayer rejecting the Send
 * before the exchange started. */
export type SendFailure =
  | { kind: "provider"; error: ProviderError }
  | { kind: "refused"; status: number; message: string };

/** The frontend's Send request: the per-call settings only (an optional
 * `imageDataUrl` for the image mode). No model is sent — the backend picks its
 * DeepSeek default. The InputMode is not here: the Session carries the one it
 * was created under. */
export interface SendRequest {
  /** #122 reasoning depth; the backend defaults to `low` when absent. */
  thinkingLevel?: ThinkingLevel;
  imageDataUrl?: string;
}

/** The ai-player slice entry point, injected via `AppDeps`. The real implementation
 * (`createAiApi`) talks to the backend AI Session routes: `begin` POSTs
 * `/ai/begin`, `send` POSTs `/ai/send` (issue #131, #133). */
export interface AiApi {
  /** Loads the AI runtime, then begins an AI Session under `mode` — `mode`'s
   * system prompt is the Session's, and `mode` stays this Session's for its
   * whole life. A load failure rejects with a `ProviderError` (see
   * `isProviderError`). */
  begin(mode: InputMode): Promise<void>;
  /** Appends the current board. A refusal or a provider failure before the
   * stream starts arrives on `onFailure`; a mid-stream failure arrives as a
   * `ReplyEvent`. */
  send(
    req: SendRequest,
    onEvent: (e: ReplyEvent) => void,
    onFailure: (f: SendFailure) => void,
  ): void;
  /** Cancels the Send in flight, if any. */
  interrupt_by_user(): Promise<unknown>;
}

/** Builds the real `AiApi` that talks to the backend AI Session routes.
 * `send` does not `abort` the SSE on interrupt — the backend emits the
 * `interrupt` event on the open stream (issue #97, #119). */
export function createAiApi(): AiApi {
  return {
    async begin(mode) {
      let res: Response;
      try {
        res = await fetch("/ai/begin", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ input_mode: mode }),
        });
      } catch (err) {
        log.error("POST /ai/begin failed", err);
        throw asProviderError(err);
      }
      if (!res.ok) {
        const providerError = await readProviderError(res);
        log.error(`POST /ai/begin failed: ${res.status}`);
        throw providerError;
      }
    },
    send(req, onEvent, onFailure) {
      void consumeEvents(req, onEvent, onFailure);
    },
    async interrupt_by_user() {
      try {
        const res = await fetch("/ai/interrupt", { method: "POST" });
        if (!res.ok) {
          log.error(`POST /ai/interrupt failed: ${res.status}`);
        }
        return res;
      } catch (err) {
        log.error("POST /ai/interrupt failed", err);
        return null;
      }
    },
  };
}

/** The frontend request body on the wire. The frontend type keeps the
 * camelCase `imageDataUrl` (issue #114), but the backend
 * `ai_adapter::SendRequest` field is snake_case `image_data_url`. */
function wireRequest(req: SendRequest): Record<string, unknown> {
  return {
    thinking_level: req.thinkingLevel,
    image_data_url: req.imageDataUrl,
  };
}

/** POSTs the Send request and forwards the SSE stream to `onEvent`. */
async function consumeEvents(
  req: SendRequest,
  onEvent: (e: ReplyEvent) => void,
  onFailure: (f: SendFailure) => void,
): Promise<void> {
  let res: Response;
  try {
    res = await fetch("/ai/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(wireRequest(req)),
    });
  } catch (err) {
    onFailure({ kind: "provider", error: asProviderError(err) });
    return;
  }
  if (!res.ok) {
    onFailure(await readFailure(res));
    return;
  }
  await consumeSse(res, onEvent);
}

/** Shapes an unknown failure as an `upstream` `ProviderError`, so the machine
 * can alert it through the same path as a Send failure. */
function asProviderError(err: unknown): ProviderError {
  return {
    kind: "upstream",
    code: null,
    message: err instanceof Error ? err.message : String(err),
  };
}

/** Narrows an unknown thrown value to a `ProviderError`. */
export function isProviderError(value: unknown): value is ProviderError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    "message" in value
  );
}

/** Parses a non-OK send response into a `SendFailure`: a `{kind,code,message}`
 * body is the Provider's failure; a `{error}` body is the AiPlayer refusing the
 * Send before it started; anything else is a transport-level provider failure
 * keyed by the status. */
async function readFailure(res: Response): Promise<SendFailure> {
  let body: unknown;
  try {
    body = await res.json();
  } catch {
    body = null;
  }
  if (body && typeof body === "object" && "error" in body) {
    return {
      kind: "refused",
      status: res.status,
      message: String((body as { error: unknown }).error),
    };
  }
  return { kind: "provider", error: providerErrorFromBody(body, res.status) };
}

/** The provider error a non-OK response body carries. A body with the backend's
 * `{kind,code,message}` shape is taken as-is; anything else is a transport-level
 * upstream failure keyed by the status (an HTML error page, a gateway body). */
function providerErrorFromBody(body: unknown, status: number): ProviderError {
  if (body && typeof body === "object" && "kind" in body) {
    return body as ProviderError;
  }
  return {
    kind: "upstream",
    code: status,
    message: `AI request failed (HTTP ${status})`,
  };
}

/** Parses a non-OK `begin` response into a `ProviderError`. */
async function readProviderError(res: Response): Promise<ProviderError> {
  let body: unknown;
  try {
    body = await res.json();
  } catch {
    body = null;
  }
  return providerErrorFromBody(body, res.status);
}

/** Reads the response body as an SSE stream and emits `ReplyEvent`s. */
async function consumeSse(
  res: Response,
  onEvent: (e: ReplyEvent) => void,
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
        onEvent(JSON.parse(data) as ReplyEvent);
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
