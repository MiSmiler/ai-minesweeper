// Tests for the real SSE `AiApi` (issue #119): `startGuide` consumes the
// backend `/ai/guide/:id` SSE stream into `GuideEvent`s and `interrupt_by_user`
// POSTs the interrupt route.

import { afterEach, describe, expect, it, vi } from "vitest";
import { createAiApi, type GuideEvent, type ProviderError } from "./api";

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

function errorResponse(status: number, payload: ProviderError): Response {
  return {
    ok: false,
    status,
    json: async () => payload,
  } as unknown as Response;
}

/** Starts a guide run and resolves with the streamed events (or pushes a
 * provider error). `startGuide` is fire-and-forget, so this bridges the async
 * work for tests. */
function collect(
  api: ReturnType<typeof createAiApi>,
  sid: string,
  req: Parameters<ReturnType<typeof createAiApi>["startGuide"]>[1],
  providerErrors: ProviderError[] = [],
): Promise<GuideEvent[]> {
  return new Promise((resolve) => {
    const events: GuideEvent[] = [];
    api.startGuide(
      sid,
      req,
      (e) => {
        events.push(e);
        // Both `sse_done` and `interrupt` terminate a run (the backend ends the
        // stream on an interrupt without a `[DONE]`).
        if (e.kind === "sse_done" || e.kind === "interrupt") resolve(events);
      },
      (e) => {
        providerErrors.push(e);
        resolve(events);
      },
    );
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("createAiApi.startGuide (SSE consumer)", () => {
  it("streams reasoning, content and [DONE] into GuideEvents", async () => {
    const api = createAiApi();
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          okResponse([
            'data: {"kind":"reasoning","text":"think"}\n\n',
            'data: {"kind":"content","text":"SUGGEST null"}\n\n',
            "data: [DONE]\n\n",
          ]),
        ),
    );

    const events = await collect(api, "s1", { format: "emoji" });

    expect(events).toEqual([
      { kind: "reasoning", text: "think" },
      { kind: "content", text: "SUGGEST null" },
      { kind: "sse_done" },
    ]);
    // The request POSTs to the session route with the format body.
    const [url, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(url).toBe("/ai/guide/s1");
    expect(JSON.parse(init!.body as string)).toEqual({ format: "emoji" });
  });

  it("parses events that straddle a chunk boundary", async () => {
    const api = createAiApi();
    // Split the content event's terminating blank line across two chunks.
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          okResponse([
            'data: {"kind":"reasoning","text":"think"}\n\n',
            'data: {"kind":"content","text":"hello world"}\n',
            "\n",
            "data: [DONE]\n\n",
          ]),
        ),
    );

    const events = await collect(api, "s1", { format: "emoji" });
    expect(events).toEqual([
      { kind: "reasoning", text: "think" },
      { kind: "content", text: "hello world" },
      { kind: "sse_done" },
    ]);
  });

  it("maps the camelCase imageDataUrl to the snake_case wire field", async () => {
    const api = createAiApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(okResponse(["data: [DONE]\n\n"])),
    );

    await collect(api, "s1", {
      format: "image",
      imageDataUrl: "data:image/png;base64,AAAA",
    });

    const [, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(JSON.parse(init!.body as string)).toEqual({
      format: "image",
      image_data_url: "data:image/png;base64,AAAA",
    });
  });

  it("parses an interrupt event with its reason", async () => {
    const api = createAiApi();
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          okResponse([
            'data: {"kind":"reasoning","text":"think"}\n\n',
            'data: {"kind":"interrupt","reason":"user_interrupt"}\n\n',
          ]),
        ),
    );

    const events = await collect(api, "s1", { format: "emoji" });
    expect(events[1]).toEqual({ kind: "interrupt", reason: "user_interrupt" });
  });

  it("hands a non-OK response to onProviderError and emits no events", async () => {
    const api = createAiApi();
    const providerErrors: ProviderError[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        errorResponse(503, {
          kind: "config",
          code: null,
          message: "no provider",
        }),
      ),
    );

    const events = await collect(
      api,
      "s1",
      { format: "emoji" },
      providerErrors,
    );
    expect(events).toEqual([]);
    expect(providerErrors).toEqual([
      { kind: "config", code: null, message: "no provider" },
    ]);
  });

  it("maps a network failure to an upstream provider error", async () => {
    const api = createAiApi();
    const providerErrors: ProviderError[] = [];
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("boom")));

    await collect(api, "s1", { format: "emoji" }, providerErrors);
    expect(providerErrors[0]?.kind).toBe("upstream");
    expect(providerErrors[0]?.message).toBe("boom");
  });
});

describe("createAiApi.interrupt_by_user", () => {
  it("POSTs to the interrupt route", async () => {
    const api = createAiApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, status: 204 } as Response),
    );

    await api.interrupt_by_user("s1");
    expect(vi.mocked(fetch)).toHaveBeenCalledWith("/ai/guide/s1/interrupt", {
      method: "POST",
    });
  });
});
