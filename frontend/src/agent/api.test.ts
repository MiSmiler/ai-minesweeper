// Tests for the Agent's transport (issue #131, #119): `messages` GETs
// `/ai/messages` and unwraps the envelope, `interrupt` POSTs `/ai/interrupt`,
// and `isProviderError` narrows the error a `begin` rejects with. The binding's
// own routes are `ai-player/api.test.ts`.

import { afterEach, describe, expect, it, vi } from "vitest";
import { createAgentApi, isProviderError } from "./api";

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

describe("isProviderError", () => {
  it("recognizes a provider error shape", () => {
    expect(isProviderError({ kind: "config", code: null, message: "x" })).toBe(
      true,
    );
    expect(isProviderError(new Error("x"))).toBe(false);
    expect(isProviderError(null)).toBe(false);
  });
});
