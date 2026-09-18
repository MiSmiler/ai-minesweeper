// Tests for the Agent's Run (issue #119, #131): the SSE body a Send answers
// with is read into `RunEvent`s here — reasoning / content deltas, the `[DONE]`
// terminator, an Interrupt and a mid-stream Provider failure. The binding's own
// half (the URL, the request body, a non-2xx failure) is `ai-player/api.test.ts`.

import { describe, expect, it } from "vitest";
import { consumeSse, isProviderError, type RunEvent } from "./run";

/** A minimal SSE body backed by a fake reader over `chunks`. */
function sseBody(chunks: string[]): {
  getReader(): {
    read(): Promise<{ value: Uint8Array; done: boolean }>;
  };
} {
  let i = 0;
  return {
    getReader() {
      return {
        async read() {
          if (i < chunks.length) {
            return {
              value: new TextEncoder().encode(chunks[i++]!),
              done: false,
            };
          }
          return { value: new Uint8Array(0), done: true };
        },
      };
    },
  };
}

function okResponse(chunks: string[]): Response {
  return {
    ok: true,
    status: 200,
    body: sseBody(chunks),
  } as unknown as Response;
}

/** Reads `chunks` as a Run stream and resolves with the events it emitted. */
async function collect(chunks: string[]): Promise<RunEvent[]> {
  const events: RunEvent[] = [];
  await consumeSse(okResponse(chunks), (e) => events.push(e));
  return events;
}

describe("consumeSse", () => {
  it("streams reasoning, content and [DONE] into RunEvents", async () => {
    const events = await collect([
      'data: {"kind":"reasoning","text":"think"}\n\n',
      'data: {"kind":"content","text":"(2,3)"}\n\n',
      "data: [DONE]\n\n",
    ]);

    expect(events).toEqual([
      { kind: "reasoning", text: "think" },
      { kind: "content", text: "(2,3)" },
      { kind: "sse_done" },
    ]);
  });

  it("parses events that straddle a chunk boundary", async () => {
    // Split the content event's terminating blank line across two chunks.
    const events = await collect([
      'data: {"kind":"reasoning","text":"think"}\n\n',
      'data: {"kind":"content","text":"hello world"}\n',
      "\n",
      "data: [DONE]\n\n",
    ]);

    expect(events).toEqual([
      { kind: "reasoning", text: "think" },
      { kind: "content", text: "hello world" },
      { kind: "sse_done" },
    ]);
  });

  it("parses an interrupted event", async () => {
    const events = await collect([
      'data: {"kind":"reasoning","text":"think"}\n\n',
      'data: {"kind":"interrupted"}\n\n',
    ]);

    expect(events[1]).toEqual({ kind: "interrupted" });
  });

  it("parses a provider_error event with the provider's own error", async () => {
    const events = await collect([
      'data: {"kind":"provider_error","error":{"kind":"upstream","code":429,"message":"rate limited"}}\n\n',
    ]);

    expect(events[0]).toEqual({
      kind: "provider_error",
      error: { kind: "upstream", code: 429, message: "rate limited" },
    });
  });
});

describe("isProviderError", () => {
  it("recognizes a provider error shape", () => {
    expect(isProviderError({ kind: "config", code: null, message: "x" })).toBe(
      true,
    );
    expect(isProviderError(new Error("x"))).toBe(false);
    expect(isProviderError(null)).toBe(false);
  });
});
