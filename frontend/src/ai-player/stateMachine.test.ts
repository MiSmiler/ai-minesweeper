// Tests for the ai-player state machine (issue #119, #133): phase transitions, the
// AI Session lifecycle (`none` / `unused` / `used`), text accumulation, and
// generation-based invalidation of stale streams.

import { describe, expect, it, vi } from "vitest";
import type { AiApi } from "./api";
import { createAiPlayerMachine, type AiPlayerState } from "./stateMachine";

/** A machine over a stubbed `AiApi`, with every state it emitted recorded. */
function makeMachine() {
  const api: AiApi = {
    begin: vi.fn(async () => {}),
    send: vi.fn(),
    interrupt_by_user: vi.fn().mockResolvedValue(undefined),
  };
  const machine = createAiPlayerMachine({ api });
  const states: AiPlayerState[] = [];
  const unsubscribe = machine.onState((s) => states.push(s));
  return { api, machine, states, unsubscribe };
}

/** The `onEvent` / `onFailure` callbacks the machine hands to the Nth Send
 * (the 2nd and 3rd arguments of `AiApi.send`). */
const onEvent = (h: ReturnType<typeof makeMachine>, n = 0) =>
  vi.mocked(h.api.send).mock.calls[n][1];
const onFailure = (h: ReturnType<typeof makeMachine>, n = 0) =>
  vi.mocked(h.api.send).mock.calls[n][2];

const last = (h: ReturnType<typeof makeMachine>): AiPlayerState =>
  h.states.at(-1)!;

describe("createAiPlayerMachine session lifecycle", () => {
  it("starts with no session and does not call the backend", () => {
    const h = makeMachine();
    expect(h.api.begin).not.toHaveBeenCalled();
    expect(h.states).toHaveLength(0);
  });

  it("send without a session is a no-op", () => {
    const h = makeMachine();
    h.machine.send({});
    expect(h.api.send).not.toHaveBeenCalled();
    expect(h.states).toHaveLength(0);
  });

  it("begin creates an unused session with the given mode", async () => {
    const h = makeMachine();
    await h.machine.begin("emoji");
    expect(h.api.begin).toHaveBeenCalledWith("emoji");
    expect(last(h).phase).toBe("idle");
    expect(last(h).sessionState).toBe("unused");
  });

  it("send marks the session used and runs", async () => {
    const h = makeMachine();
    await h.machine.begin("plain");
    h.machine.send({});
    const s = last(h);
    expect(s.phase).toBe("running");
    expect(s.sessionState).toBe("used");
    expect(h.api.send).toHaveBeenCalledTimes(1);
    expect(h.api.send).toHaveBeenCalledWith(
      {},
      expect.any(Function),
      expect.any(Function),
    );
  });

  it("a second send after a reply runs again", async () => {
    const h = makeMachine();
    await h.machine.begin("plain");
    h.machine.send({});
    onEvent(h)({ kind: "sse_done" });
    h.machine.send({});
    expect(h.api.send).toHaveBeenCalledTimes(2);
    expect(last(h).phase).toBe("running");
  });

  it("end returns to none and clears the text", async () => {
    const h = makeMachine();
    await h.machine.begin("plain");
    h.machine.send({});
    onEvent(h)({ kind: "content", text: "early" });
    h.machine.end();
    const s = last(h);
    expect(s.phase).toBe("idle");
    expect(s.sessionState).toBe("none");
    expect(s.content).toBe("");
    // The session is gone, so a later send is a no-op.
    h.machine.send({});
    expect(h.api.send).toHaveBeenCalledTimes(1);
  });

  it("a failed begin surfaces a provider failure and leaves no session", async () => {
    const h = makeMachine();
    vi.mocked(h.api.begin).mockRejectedValueOnce(new Error("offline"));
    await h.machine.begin("plain");
    const s = last(h);
    expect(s.phase).toBe("failed");
    expect(s.sessionState).toBe("none");
    expect(s.failure).toEqual({
      kind: "provider",
      error: { kind: "upstream", code: null, message: "offline" },
    });
  });

  it("preserves a ProviderError rejected by begin", async () => {
    const h = makeMachine();
    vi.mocked(h.api.begin).mockRejectedValueOnce({
      kind: "config",
      code: null,
      message: "no key",
    });
    await h.machine.begin("plain");
    expect(last(h).failure).toEqual({
      kind: "provider",
      error: { kind: "config", code: null, message: "no key" },
    });
    expect(last(h).sessionState).toBe("none");
  });

  it("ignores a begin superseded by end", async () => {
    const h = makeMachine();
    let resolve!: () => void;
    vi.mocked(h.api.begin).mockReturnValueOnce(
      new Promise((r) => {
        resolve = r;
      }),
    );
    const pending = h.machine.begin("plain");
    h.machine.end();
    resolve();
    await pending;
    expect(last(h).sessionState).toBe("none");
    h.machine.send({});
    expect(h.api.send).not.toHaveBeenCalled();
  });
});

describe("createAiPlayerMachine run transitions", () => {
  async function emptySession() {
    const h = makeMachine();
    await h.machine.begin("plain");
    return h;
  }

  it("accumulates reasoning and content streams", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({ kind: "reasoning", text: "a" });
    onEvent(h)({ kind: "content", text: "hi" });
    onEvent(h)({ kind: "reasoning", text: "b" });
    onEvent(h)({ kind: "content", text: " (2,3)" });

    const s = last(h);
    expect(s.phase).toBe("running");
    expect(s.reasoning).toBe("ab");
    expect(s.content).toBe("hi (2,3)");
  });

  it("sse_done completes the run and keeps the session used", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({ kind: "sse_done" });
    const s = last(h);
    expect(s.phase).toBe("done");
    expect(s.sessionState).toBe("used");
  });

  it("an interrupted send keeps the session used", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({ kind: "interrupted" });
    const s = last(h);
    expect(s.phase).toBe("interrupted");
    expect(s.sessionState).toBe("used");
  });

  it("a later interrupted send keeps the session used", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({ kind: "sse_done" });
    h.machine.send({});
    onEvent(h, 1)({ kind: "interrupted" });
    expect(last(h).sessionState).toBe("used");
  });

  it("a send failure enters failed and keeps the session used", async () => {
    const h = await emptySession();
    h.machine.send({});
    onFailure(h)({
      kind: "provider",
      error: { kind: "config", code: null, message: "no" },
    });
    const s = last(h);
    expect(s.phase).toBe("failed");
    expect(s.sessionState).toBe("used");
    expect(s.failure).toEqual({
      kind: "provider",
      error: { kind: "config", code: null, message: "no" },
    });
  });

  it("a mid-stream provider_error event fails with the provider failure", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({
      kind: "provider_error",
      error: { kind: "upstream", code: 429, message: "rate limited" },
    });
    const s = last(h);
    expect(s.phase).toBe("failed");
    expect(s.failure).toEqual({
      kind: "provider",
      error: { kind: "upstream", code: 429, message: "rate limited" },
    });
  });

  it("a refused failure enters failed with the refusal", async () => {
    const h = await emptySession();
    h.machine.send({});
    onFailure(h)({ kind: "refused", status: 409, message: "busy" });
    const s = last(h);
    expect(s.phase).toBe("failed");
    expect(s.failure).toEqual({
      kind: "refused",
      status: 409,
      message: "busy",
    });
  });

  it("end invalidates a stale in-flight stream", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({ kind: "content", text: "early" });
    h.machine.end();
    expect(last(h).phase).toBe("idle");
    expect(last(h).content).toBe("");

    // A late event from the superseded run must not corrupt the state.
    onEvent(h)({ kind: "content", text: "late" });
    expect(last(h).phase).toBe("idle");
    expect(last(h).content).toBe("");
    onFailure(h)({
      kind: "provider",
      error: { kind: "upstream", code: 500, message: "x" },
    });
    expect(last(h).phase).toBe("idle");
  });

  it("interrupt_by_user cancels the live session", async () => {
    const h = await emptySession();
    h.machine.send({});
    await h.machine.interrupt_by_user();
    expect(h.api.interrupt_by_user).toHaveBeenCalledTimes(1);
  });

  it("interrupt_by_user without a session is a no-op", async () => {
    const h = makeMachine();
    await h.machine.interrupt_by_user();
    expect(h.api.interrupt_by_user).not.toHaveBeenCalled();
  });

  it("captures the user message event into state.user", async () => {
    const h = await emptySession();
    h.machine.send({});
    onEvent(h)({ kind: "user", text: "........." });
    expect(last(h).user).toBe(".........");
  });

  it("seeds userImageUrl from the request for the image mode", async () => {
    const h = await emptySession();
    h.machine.send({
      imageDataUrl: "data:image/png;base64,AAAA",
    });
    const s = last(h);
    expect(s.userImageUrl).toBe("data:image/png;base64,AAAA");
    expect(s.user).toBe("");
  });

  it("leaves userImageUrl unset for a text mode", async () => {
    const h = await emptySession();
    h.machine.send({});
    expect(last(h).userImageUrl).toBeUndefined();
  });

  it("onState unsubscribe stops notifications", async () => {
    const h = makeMachine();
    h.unsubscribe();
    await h.machine.begin("plain");
    expect(h.states).toHaveLength(0);
  });
});
