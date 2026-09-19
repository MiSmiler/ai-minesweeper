// The ai-player binding's frontend half (issue #114, #131, #133): the two
// things the binding owns in the browser — the InputMode a Session is created
// under, and the aux send that appends the current board to it. The other
// routes address the Agent, not the binding (`agent/api.ts`).
//
// `begin` POSTs `/ai/begin`; `auxSend` POSTs `/ai/aux-send` with the per-call
// settings and hands the response body to the Agent's `consumeSse`
// (`agent/run.ts`), because the stream an aux send produces is that Agent's
// Run. The binding contributes the URL, the request body and the board
// rendering — never the events.
//
// `auxSend` does not `abort` the SSE on interrupt: the backend emits the
// `interrupt` event on the open stream (issue #97, #119).

import { asProviderError, consumeSse } from "../agent/run";
import type { ProviderError, RunEvent, RunFailure } from "../agent/run";
import { log } from "../infra/log";

/** The input modes, mirrored from the backend kebab-case
 * (`ai_adapter::InputMode`). "model is a provider-specific name string, not
 * a literal union" — the model is never sent by the frontend. */
export type InputMode = "plain" | "emoji" | "image";

/** The #122 reasoning-depth control: `off` disables thinking mode; the rest
 * set the `reasoning_effort`. Mirrored from `ai_adapter::ThinkingLevel`. */
export type ThinkingLevel = "off" | "low" | "high" | "max";

/** The frontend's aux send request: the per-call settings only (an optional
 * `imageDataUrl` for the image mode). No model is sent — the backend picks its
 * DeepSeek default. The InputMode is not here: the Session carries the one it
 * was created under. */
export interface AuxSendRequest {
  /** #122 reasoning depth; the backend defaults to `low` when absent. */
  thinkingLevel?: ThinkingLevel;
  imageDataUrl?: string;
}

/** The ai-player slice entry point, injected via `AppDeps`. The real
 * implementation (`createAiPlayerApi`) talks to the backend's binding routes:
 * `begin` POSTs `/ai/begin`, `auxSend` POSTs `/ai/aux-send` (issue #131, #133). */
export interface AiPlayerApi {
  /** Loads the AI runtime, then begins an AI Session under `mode` — `mode`'s
   * system prompt is the Session's, and `mode` stays this Session's for its
   * whole life. A load failure rejects with a `ProviderError`. */
  begin(mode: InputMode): Promise<void>;
  /** Appends the current board. A refusal or a provider failure before the
   * stream starts arrives on `onFailure`; a mid-stream failure arrives as a
   * `RunEvent`. */
  auxSend(
    req: AuxSendRequest,
    onEvent: (e: RunEvent) => void,
    onFailure: (f: RunFailure) => void,
  ): void;
}

/** Builds the real `AiPlayerApi` that talks to the backend's binding routes. */
export function createAiPlayerApi(): AiPlayerApi {
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
    auxSend(req, onEvent, onFailure) {
      void consumeEvents(req, onEvent, onFailure);
    },
  };
}

/** The frontend request body on the wire. The frontend type keeps the
 * camelCase `imageDataUrl` (issue #114), but the backend's `AuxSendRequest`
 * field is snake_case `image_data_url`. */
function wireRequest(req: AuxSendRequest): Record<string, unknown> {
  return {
    thinking_level: req.thinkingLevel,
    image_data_url: req.imageDataUrl,
  };
}

/** POSTs the aux send request and forwards the SSE stream to `onEvent`. */
async function consumeEvents(
  req: AuxSendRequest,
  onEvent: (e: RunEvent) => void,
  onFailure: (f: RunFailure) => void,
): Promise<void> {
  let res: Response;
  try {
    res = await fetch("/ai/aux-send", {
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

/** Parses a non-OK aux send response into a `RunFailure`: a `{kind,code,message}`
 * body is the Provider's failure; a `{error}` body is the AiPlayer refusing the
 * aux send before it started; anything else is a transport-level provider
 * failure keyed by the status. */
async function readFailure(res: Response): Promise<RunFailure> {
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
