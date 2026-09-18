// The Agent's Run (issue #114, #131): the wire events the frontend shares with
// the backend's `RunEvent` ([src/server/ai_routes.rs](/src/server/ai_routes.rs)),
// the failure a Run reports, and the SSE reading that turns a Send's response
// body into `RunEvent`s.
//
// The file takes the concept, not the mechanism: the stream a Send opens is the
// Agent's Run — the deltas arrive on it, the reply lands on it, and it ends on
// it. The Send's own transport lives in the binding (`ai-player/api.ts`): it
// owns the URL and the request body.

import { log } from "../infra/log";

/** A frontend-consumed Run event. `sse_done` is synthesized locally when the
 * SSE `[DONE]` is read; the wire never emits it. `user` is the backend echo of
 * the player's message (issue #124), emitted first on the stream.
 * `interrupted` is the caller's own Interrupt; `provider_error` carries the
 * Provider's own failure. */
export type RunEvent =
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

/** Why the run produced no reply, as the frontend sees it. A `provider` failure
 * is the Provider's own cause, wherever it happened (Load, delivery or
 * mid-stream), and carries it intact; a `refused` failure is a Send the
 * AiPlayer or the Agent rejected before the exchange started. The positions are
 * not split here the way the Rust types split them: one field (`run.failure`)
 * receives every path, and TypeScript cannot enforce the restriction anyway. */
export type RunFailure =
  | { kind: "provider"; error: ProviderError }
  | { kind: "refused"; status: number; message: string };

/** Narrows an unknown thrown value to a `ProviderError`. */
export function isProviderError(value: unknown): value is ProviderError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    "message" in value
  );
}

/** Shapes an unknown thrown value as a `ProviderError`: one already shaped is
 * kept intact (the backend's own cause, with its kind and code), anything else
 * becomes an `upstream` failure carrying its message. */
export function asProviderError(err: unknown): ProviderError {
  if (isProviderError(err)) return err;
  return {
    kind: "upstream",
    code: null,
    message: err instanceof Error ? err.message : String(err),
  };
}

/** Reads a Run's response body as an SSE stream and emits `RunEvent`s. The
 * caller owns the URL and the request body; this owns the stream's reading.
 * Each `data:` payload is a `RunEvent` JSON or the `[DONE]` terminator. */
export async function consumeSse(
  res: Response,
  onEvent: (e: RunEvent) => void,
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
        onEvent(JSON.parse(data) as RunEvent);
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
