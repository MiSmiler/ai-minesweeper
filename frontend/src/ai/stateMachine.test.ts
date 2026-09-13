// Tests for the guide state machine (issue #119, #133): phase transitions, the
// AI Session lifecycle (`none` / `empty` / `non-empty`), text accumulation, and
// generation-based invalidation of stale streams.

import { describe, expect, it, vi } from "vitest";
import type { GuideEvent, ProviderError } from "./api";
import { createGuideMachine, type GuideState } from "./stateMachine";

interface Harness {
  api: {
    createSession: ReturnType<typeof vi.fn>;
    send: ReturnType<typeof vi.fn>;
    interrupt_by_user: ReturnType<typeof vi.fn>;
  };
  handlers: Array<{
    onEvent: (e: GuideEvent) => void;
    onProviderError: (e: ProviderError) => void;
  }>;
  machine: ReturnType<typeof createGuideMachine>;
  states: GuideState[];
  unsubscribe: () => void;
}

function setup(): Harness {
  const handlers: Harness["handlers"] = [];
  let seq = 0;
  const api = {
    createSession: vi.fn(async () => ({ sessionId: `session-${seq++}` })),
    send: vi.fn(
      (
        _sid: string,
        _req: unknown,
        onEvent: (e: GuideEvent) => void,
        onProviderError: (e: ProviderError) => void,
      ) => {
        handlers.push({ onEvent, onProviderError });
      },
    ),
    interrupt_by_user: vi.fn().mockResolvedValue(undefined),
  };
  const machine = createGuideMachine({ api });
  const states: GuideState[] = [];
  const unsubscribe = machine.onState((s) => states.push(s));
  return { api, handlers, machine, states, unsubscribe };
}

const last = (h: Harness): GuideState => h.states.at(-1)!;

describe("createGuideMachine session lifecycle", () => {
  it("starts with no session and does not call the backend", () => {
    const h = setup();
    expect(h.api.createSession).not.toHaveBeenCalled();
    expect(h.states).toHaveLength(0);
  });

  it("send without a session is a no-op", () => {
    const h = setup();
    h.machine.send({ inputMode: "plain" });
    expect(h.api.send).not.toHaveBeenCalled();
    expect(h.states).toHaveLength(0);
  });

  it("newSession creates an empty session", async () => {
    const h = setup();
    await h.machine.newSession();
    expect(h.api.createSession).toHaveBeenCalledTimes(1);
    expect(last(h).phase).toBe("idle");
    expect(last(h).sessionState).toBe("empty");
  });

  it("send appends to the current session and runs", async () => {
    const h = setup();
    await h.machine.newSession();
    h.machine.send({ inputMode: "emoji" });
    const s = last(h);
    expect(s.phase).toBe("running");
    expect(s.sessionState).toBe("empty");
    expect(h.api.send).toHaveBeenCalledTimes(1);
    expect(h.api.send).toHaveBeenCalledWith(
      "session-0",
      { inputMode: "emoji" },
      expect.any(Function),
      expect.any(Function),
    );
  });

  it("a second send reuses the same session id", async () => {
    const h = setup();
    await h.machine.newSession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "sse_done" });
    h.machine.send({ inputMode: "emoji" });
    expect(h.api.send.mock.calls[1]![0]).toBe("session-0");
  });

  it("endSession returns to none and clears the text", async () => {
    const h = setup();
    await h.machine.newSession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "content", text: "early" });
    h.machine.endSession();
    const s = last(h);
    expect(s.phase).toBe("idle");
    expect(s.sessionState).toBe("none");
    expect(s.content).toBe("");
    // The session is gone, so a later send is a no-op.
    h.machine.send({ inputMode: "emoji" });
    expect(h.api.send).toHaveBeenCalledTimes(1);
  });

  it("a failed newSession surfaces preflight-failed and leaves no session", async () => {
    const h = setup();
    h.api.createSession.mockRejectedValueOnce(new Error("offline"));
    await h.machine.newSession();
    const s = last(h);
    expect(s.phase).toBe("preflight-failed");
    expect(s.sessionState).toBe("none");
    expect(s.providerError).toEqual({
      kind: "upstream",
      code: null,
      message: "offline",
    });
  });

  it("ignores a newSession response superseded by endSession", async () => {
    const h = setup();
    let resolve!: (v: { sessionId: string }) => void;
    h.api.createSession.mockReturnValueOnce(
      new Promise((r) => {
        resolve = r;
      }),
    );
    const pending = h.machine.newSession();
    h.machine.endSession();
    resolve({ sessionId: "late" });
    await pending;
    expect(last(h).sessionState).toBe("none");
    h.machine.send({ inputMode: "emoji" });
    expect(h.api.send).not.toHaveBeenCalled();
  });
});

describe("createGuideMachine run transitions", () => {
  async function emptySession(): Promise<Harness> {
    const h = setup();
    await h.machine.newSession();
    return h;
  }

  it("accumulates reasoning and content streams", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "reasoning", text: "a" });
    h.handlers[0]!.onEvent({ kind: "content", text: "hi" });
    h.handlers[0]!.onEvent({ kind: "reasoning", text: "b" });
    h.handlers[0]!.onEvent({ kind: "content", text: " (2,3)" });

    const s = last(h);
    expect(s.phase).toBe("running");
    expect(s.reasoning).toBe("ab");
    expect(s.content).toBe("hi (2,3)");
  });

  it("sse_done completes the run and marks the session non-empty", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "sse_done" });
    const s = last(h);
    expect(s.phase).toBe("done");
    expect(s.sessionState).toBe("non-empty");
  });

  it("an interrupted first send leaves the session empty", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "interrupt", reason: "user_interrupt" });
    const s = last(h);
    expect(s.phase).toBe("interrupted");
    expect(s.interruptReason).toBe("user_interrupt");
    expect(s.sessionState).toBe("empty");
  });

  it("an interrupted later send keeps the session non-empty", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "sse_done" });
    h.machine.send({ inputMode: "emoji" });
    h.handlers[1]!.onEvent({ kind: "interrupt", reason: "user_interrupt" });
    expect(last(h).sessionState).toBe("non-empty");
  });

  it("a provider error enters preflight-failed without changing the session", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onProviderError({
      kind: "config",
      code: null,
      message: "no",
    });
    const s = last(h);
    expect(s.phase).toBe("preflight-failed");
    expect(s.sessionState).toBe("empty");
    expect(s.providerError).toEqual({
      kind: "config",
      code: null,
      message: "no",
    });
  });

  it("endSession invalidates a stale in-flight stream", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "content", text: "early" });
    h.machine.endSession();
    expect(last(h).phase).toBe("idle");
    expect(last(h).content).toBe("");

    // A late event from the superseded run must not corrupt the state.
    h.handlers[0]!.onEvent({ kind: "content", text: "late" });
    expect(last(h).phase).toBe("idle");
    expect(last(h).content).toBe("");
    h.handlers[0]!.onProviderError({
      kind: "upstream",
      code: 500,
      message: "x",
    });
    expect(last(h).phase).toBe("idle");
  });

  it("interrupt_by_user targets the current session", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    await h.machine.interrupt_by_user();
    expect(h.api.interrupt_by_user).toHaveBeenCalledWith("session-0");
  });

  it("interrupt_by_user without a session is a no-op", async () => {
    const h = setup();
    await h.machine.interrupt_by_user();
    expect(h.api.interrupt_by_user).not.toHaveBeenCalled();
  });

  it("captures the user message event into state.user", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    h.handlers[0]!.onEvent({ kind: "user", text: "........." });
    expect(last(h).user).toBe(".........");
  });

  it("seeds userImageUrl from the request for the image mode", async () => {
    const h = await emptySession();
    h.machine.send({
      inputMode: "image",
      imageDataUrl: "data:image/png;base64,AAAA",
    });
    const s = last(h);
    expect(s.userImageUrl).toBe("data:image/png;base64,AAAA");
    expect(s.user).toBe("");
  });

  it("leaves userImageUrl unset for a text mode", async () => {
    const h = await emptySession();
    h.machine.send({ inputMode: "emoji" });
    expect(last(h).userImageUrl).toBeUndefined();
  });

  it("onState unsubscribe stops notifications", async () => {
    const h = setup();
    h.unsubscribe();
    await h.machine.newSession();
    expect(h.states).toHaveLength(0);
  });
});
