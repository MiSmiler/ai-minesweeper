// The Agent's Session transport (issue #114, #131): the wire types the frontend
// shares with the backend's `Message`
// ([src/server/ai_routes.rs](/src/server/ai_routes.rs)), and the two routes that
// address the Agent itself — `messages` GETs `/ai/messages` for the live
// Session's message list, `interrupt` POSTs `/ai/interrupt`.
//
// The Run's events and failure live in `agent/run.ts`; the Send's URL and
// request body live in the binding (`ai-player/api.ts`).

import { log } from "../infra/log";

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
  /** Cancels the Run in flight, if any. `void`: the backend always answers
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
