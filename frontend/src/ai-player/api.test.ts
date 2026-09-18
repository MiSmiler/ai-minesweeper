// Tests for the binding's frontend half (issue #119, #133): `begin` POSTs
// `/ai/begin`, and `send` POSTs `/ai/send` and hands the response body to the
// Agent's Run reader (`agent/run.test.ts`), with a non-2xx handed to
// `onFailure`. The routes that address the Agent are `agent/api.test.ts`.

import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderError, RunEvent, RunFailure } from "../agent/run";
import { createAiPlayerApi } from "./api";

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

/** Starts a Send and resolves with the streamed events (or pushes a failure).
 * `send` is fire-and-forget, so this bridges the async work for tests. */
function collect(
  api: ReturnType<typeof createAiPlayerApi>,
  req: Parameters<ReturnType<typeof createAiPlayerApi>["send"]>[0],
  failures: RunFailure[] = [],
): Promise<RunEvent[]> {
  return new Promise((resolve) => {
    const events: RunEvent[] = [];
    api.send(
      req,
      (e) => {
        events.push(e);
        // `sse_done`, `interrupted` and `provider_error` all terminate a run
        // (the backend ends the stream on a failure without a `[DONE]`).
        if (
          e.kind === "sse_done" ||
          e.kind === "interrupted" ||
          e.kind === "provider_error"
        ) {
          resolve(events);
        }
      },
      (f) => {
        failures.push(f);
        resolve(events);
      },
    );
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("createAiPlayerApi.send (request)", () => {
  it("POSTs /ai/send and hands the response body to the Run reader", async () => {
    const api = createAiPlayerApi();
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          okResponse([
            'data: {"kind":"reasoning","text":"think"}\n\n',
            'data: {"kind":"content","text":"(2,3)"}\n\n',
            "data: [DONE]\n\n",
          ]),
        ),
    );

    const events = await collect(api, {});

    expect(events).toEqual([
      { kind: "reasoning", text: "think" },
      { kind: "content", text: "(2,3)" },
      { kind: "sse_done" },
    ]);
    // The request POSTs to the session route; the InputMode is the Session's,
    // so it is not in the body.
    const [url, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(url).toBe("/ai/send");
    expect(JSON.parse(init!.body as string)).toEqual({});
  });

  it("maps the camelCase imageDataUrl to the snake_case wire field", async () => {
    const api = createAiPlayerApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(okResponse(["data: [DONE]\n\n"])),
    );

    await collect(api, {
      imageDataUrl: "data:image/png;base64,AAAA",
    });

    const [, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(JSON.parse(init!.body as string)).toEqual({
      image_data_url: "data:image/png;base64,AAAA",
    });
  });

  it("maps the camelCase thinkingLevel to the snake_case wire field", async () => {
    const api = createAiPlayerApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(okResponse(["data: [DONE]\n\n"])),
    );

    await collect(api, { thinkingLevel: "high" });

    const [, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(JSON.parse(init!.body as string)).toEqual({
      thinking_level: "high",
    });
  });

  it("hands a provider-body failure to onFailure and emits no events", async () => {
    const api = createAiPlayerApi();
    const failures: RunFailure[] = [];
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

    const events = await collect(api, {}, failures);
    expect(events).toEqual([]);
    expect(failures).toEqual([
      {
        kind: "provider",
        error: { kind: "config", code: null, message: "no provider" },
      },
    ]);
  });

  it("hands a refusal body to onFailure as a refusal", async () => {
    const api = createAiPlayerApi();
    const failures: RunFailure[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 409,
        json: async () => ({
          error: "a send is already in flight",
        }),
      } as unknown as Response),
    );

    await collect(api, {}, failures);
    expect(failures).toEqual([
      {
        kind: "refused",
        status: 409,
        message: "a send is already in flight",
      },
    ]);
  });

  it("treats a non-JSON error body as an upstream provider failure", async () => {
    const api = createAiPlayerApi();
    const failures: RunFailure[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 502,
        json: async () => {
          throw new Error("not json");
        },
      } as unknown as Response),
    );

    await collect(api, {}, failures);
    expect(failures).toEqual([
      {
        kind: "provider",
        error: {
          kind: "upstream",
          code: 502,
          message: "AI request failed (HTTP 502)",
        },
      },
    ]);
  });

  it("maps a network failure to an upstream provider failure", async () => {
    const api = createAiPlayerApi();
    const failures: RunFailure[] = [];
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("boom")));

    await collect(api, {}, failures);
    expect(failures[0]).toEqual({
      kind: "provider",
      error: { kind: "upstream", code: null, message: "boom" },
    });
  });
});

describe("createAiPlayerApi.begin", () => {
  it("POSTs /ai/begin and resolves with no session id", async () => {
    const api = createAiPlayerApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, status: 204 } as Response),
    );

    await expect(api.begin("plain")).resolves.toBeUndefined();
    expect(vi.mocked(fetch)).toHaveBeenCalledWith("/ai/begin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ input_mode: "plain" }),
    });
  });

  it("rejects with the parsed ProviderError body on a non-OK response", async () => {
    const api = createAiPlayerApi();
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
    await expect(api.begin("plain")).rejects.toEqual({
      kind: "config",
      code: null,
      message: "no provider",
    });
  });

  it("shapes a network failure as an upstream ProviderError", async () => {
    const api = createAiPlayerApi();
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    await expect(api.begin("plain")).rejects.toEqual({
      kind: "upstream",
      code: null,
      message: "offline",
    });
  });
});
