// The Agent's Session transport (issue #114, #131): the wire types the frontend
// shares with the backend's `ReplyEvent` and `Message`
// ([src/server/ai_routes.rs](/src/server/ai_routes.rs)), and the two routes that
// address the Agent itself — `messages` GETs `/ai/messages` for the live
// Session's message list, `interrupt` POSTs `/ai/interrupt`.
//
// The Send's own transport lives in the binding (`ai-player/api.ts`): it owns
// the URL and the request body. What it hands back is the SSE response body,
// read by `consumeSse` below — the stream a Send produces is this Agent's Run,
// so reading it belongs here.

import { log } from "../infra/log";

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

/** One entry in a Session's message list, mirroring the agent's `Message`
 * (tagged on the wire by `role`). The field names stay the provider's
 * (`reasoning_content`, `tool_call_id`): the content blocks are the provider's
 * shape, so one JSON mixing two naming styles would be worse. An optional field
 * is absent rather than `null` when unset. */
export type Message =
  | { role: "system"; content: string }
  | { role: "user"; content: ContentBlock[] }
  | {
      role: "assistant";
      content: string;
      reasoning_content?: string;
      tool_calls?: ToolCall[];
      interrupt?: boolean;
    }
  | { role: "tool"; tool_call_id: string; content: string };

/** A `user` message's content block, mirroring `ContentBlock`'s wire shape
 * (`{type:"text",text}` / `{type:"image_url",image_url:{url,detail}}`). */
export type ContentBlock =
  | { type: "text"; text: string }
  | { type: "image_url"; image_url: { url: string; detail: string } };

/** Mirrors `agent::ToolCall`. */
export interface ToolCall {
  id: string;
  name: string;
  arguments: unknown;
}

/** The `GET /ai/messages` body: the Session's message list, oldest first. */
interface MessagesDto {
  messages: Message[];
}

/** The Agent's slice entry point, injected via `AppDeps`: the live Session's
 * message list and its Interrupt. It mirrors the routes that address the Agent
 * itself; the binding's Send is the other half (`ai-player/api.ts`). */
export interface AgentApi {
  /** The live Session's message list, oldest first. `[]` when the read fails:
   * the failure is logged, never alerted — it is not a Send failure. */
  messages(): Promise<Message[]>;
  /** Cancels the Send in flight, if any. `void`: the backend always answers
   * 204, and a Send that is not in flight is not an error. */
  interrupt(): Promise<void>;
}

/** Builds the real `AgentApi` that talks to the backend's Agent routes. */
export function createAgentApi(): AgentApi {
  return {
    async messages() {
      try {
        const res = await fetch("/ai/messages");
        if (!res.ok) {
          log.error(`GET /ai/messages failed: ${res.status}`);
          return [];
        }
        return ((await res.json()) as MessagesDto).messages;
      } catch (err) {
        log.error("GET /ai/messages failed", err);
        return [];
      }
    },
    async interrupt() {
      try {
        const res = await fetch("/ai/interrupt", { method: "POST" });
        if (!res.ok) {
          log.error(`POST /ai/interrupt failed: ${res.status}`);
        }
      } catch (err) {
        log.error("POST /ai/interrupt failed", err);
      }
    },
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

/** Reads a Send's response body as an SSE stream and emits `ReplyEvent`s. The
 * caller owns the URL and the request body; this owns the stream's reading.
 * Each `data:` payload is a `ReplyEvent` JSON or the `[DONE]` terminator. */
export async function consumeSse(
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
