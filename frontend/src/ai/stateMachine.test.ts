// Tests for the guide state machine (issue #119): phase transitions, text
// accumulation, generation-based invalidation of stale streams, and reset.

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GuideEvent, ProviderError } from "./api";
import { createGuideMachine, type GuideState } from "./stateMachine";

let sidCounter = 0;
const newSessionId = (): string => `session-${sidCounter++}`;

interface Harness {
  api: {
    startGuide: ReturnType<typeof vi.fn>;
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
  const api = {
    startGuide: vi.fn(
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
  const machine = createGuideMachine({ api, newSessionId });
  const states: GuideState[] = [];
  const unsubscribe = machine.onState((s) => states.push(s));
  return { api, handlers, machine, states, unsubscribe };
}

beforeEach(() => {
  sidCounter = 0;
});

describe("createGuideMachine phase transitions", () => {
  it("start moves to running and clears prior text", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    const s = h.states.at(-1)!;
    expect(s.phase).toBe("running");
    expect(s.reasoning).toBe("");
    expect(s.content).toBe("");
    expect(h.api.startGuide).toHaveBeenCalledTimes(1);
    expect(h.api.startGuide).toHaveBeenCalledWith(
      expect.stringMatching(/^session-/),
      { format: "emoji" },
      expect.any(Function),
      expect.any(Function),
    );
  });

  it("accumulates reasoning and content streams", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    h.handlers[0]!.onEvent({ kind: "reasoning", text: "a" });
    h.handlers[0]!.onEvent({ kind: "content", text: "hi" });
    h.handlers[0]!.onEvent({ kind: "reasoning", text: "b" });
    h.handlers[0]!.onEvent({ kind: "content", text: " SUGGEST null" });

    const s = h.states.at(-1)!;
    expect(s.phase).toBe("running");
    expect(s.reasoning).toBe("ab");
    expect(s.content).toBe("hi SUGGEST null");
  });

  it("sse_done completes the run", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    h.handlers[0]!.onEvent({ kind: "sse_done" });
    expect(h.states.at(-1)!.phase).toBe("done");
  });

  it("interrupt enters interrupted with the reason", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    h.handlers[0]!.onEvent({ kind: "interrupt", reason: "rate_limit" });
    const s = h.states.at(-1)!;
    expect(s.phase).toBe("interrupted");
    expect(s.interruptReason).toBe("rate_limit");
  });

  it("a provider error enters preflight-failed with the error", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    h.handlers[0]!.onProviderError({
      kind: "config",
      code: null,
      message: "no",
    });
    const s = h.states.at(-1)!;
    expect(s.phase).toBe("preflight-failed");
    expect(s.providerError).toEqual({
      kind: "config",
      code: null,
      message: "no",
    });
  });

  it("reset returns to idle and invalidates a stale in-flight stream", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    h.handlers[0]!.onEvent({ kind: "content", text: "early" });
    h.machine.reset();
    expect(h.states.at(-1)!.phase).toBe("idle");
    expect(h.states.at(-1)!.content).toBe("");

    // A late event from the superseded run must not corrupt the state.
    h.handlers[0]!.onEvent({ kind: "content", text: "late" });
    expect(h.states.at(-1)!.phase).toBe("idle");
    expect(h.states.at(-1)!.content).toBe("");
    h.handlers[0]!.onProviderError({
      kind: "upstream",
      code: 500,
      message: "x",
    });
    expect(h.states.at(-1)!.phase).toBe("idle");
  });

  it("uses a fresh session id per start", () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    const first = h.api.startGuide.mock.calls[0]![0] as string;
    h.machine.start({ format: "emoji" });
    const second = h.api.startGuide.mock.calls[1]![0] as string;
    expect(first).not.toBe(second);
  });

  it("interrupt_by_user targets the current session", async () => {
    const h = setup();
    h.machine.start({ format: "emoji" });
    const sid = h.api.startGuide.mock.calls[0]![0] as string;
    await h.machine.interrupt_by_user();
    expect(h.api.interrupt_by_user).toHaveBeenCalledWith(sid);
  });

  it("onState unsubscribe stops notifications", () => {
    const h = setup();
    h.unsubscribe();
    h.machine.start({ format: "emoji" });
    expect(h.states).toHaveLength(0);
  });
});
