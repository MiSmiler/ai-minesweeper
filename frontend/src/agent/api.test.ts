// Tests for the Agent's transport (issue #131, #119): `messages` GETs
// `/ai/messages` and unwraps the envelope, and `interrupt` POSTs
// `/ai/interrupt`. The Run's reading is `run.test.ts`; the binding's own routes
// are `ai-player/api.test.ts`.

import { afterEach, describe, expect, it, vi } from "vitest";
import { createAgentApi } from "./api";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("createAgentApi.messages", () => {
  it("GETs /ai/messages and unwraps the envelope", async () => {
    const api = createAgentApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({
          messages: [{ role: "system", content: "be helpful" }],
        }),
      } as unknown as Response),
    );

    await expect(api.messages()).resolves.toEqual([
      { role: "system", content: "be helpful" },
    ]);
    expect(vi.mocked(fetch)).toHaveBeenCalledWith("/ai/messages");
  });

  it("resolves with an empty list on a non-OK response", async () => {
    const api = createAgentApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 409,
        json: async () => ({ error: "no live AI session" }),
      } as unknown as Response),
    );

    await expect(api.messages()).resolves.toEqual([]);
  });

  it("resolves with an empty list when the request fails", async () => {
    const api = createAgentApi();
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));

    await expect(api.messages()).resolves.toEqual([]);
  });
});

describe("createAgentApi.interrupt", () => {
  it("POSTs to the interrupt route with no id", async () => {
    const api = createAgentApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, status: 204 } as Response),
    );

    await api.interrupt();
    expect(vi.mocked(fetch)).toHaveBeenCalledWith("/ai/interrupt", {
      method: "POST",
    });
  });
});
