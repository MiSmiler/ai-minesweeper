// Tests for the agent state machine (issue #119, #133): phase transitions, the
// AI Session lifecycle (`none` / `unused` / `used`), text accumulation, and
// generation-based invalidation of stale streams. The machine addresses no
// transport — the harness plays the composition root and hands it `open` and
// `start`, so nothing here knows a URL or builds a request.

import { describe, expect, it } from "vitest";
import type { Message } from "./api";
import type { RunEvent, RunFailure } from "./run";
import { createAgentMachine, type AgentState } from "./machine";

/** Lets the machine's own awaits settle, so a state emitted after one is visible. */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/** A machine plus the two calls the composition root would hand it. `opener`
 * is what `begin` runs — a test replaces one of the two to hold it open or
 * make it fail — and `sends` records the callbacks each Send was started
 * with. */
function makeMachine() {
  const machine = createAgentMachine();
  const states: AgentState[] = [];
  const unsubscribe = machine.onState((s) => states.push(s));

  const opener = {
    begin: async (): Promise<void> => {},
    read: async (): Promise<Message[]> => [],
  };
  const sends: {
    onEvent: (e: RunEvent) => void;
    onFailure: (f: RunFailure) => void;
  }[] = [];

  return {
    machine,
    states,
    unsubscribe,
    opener,
    sends,
    /** `machine.begin` over the harness's opener. */
    begin: () =>
      machine.begin({ begin: () => opener.begin(), read: () => opener.read() }),
    /** `machine.beginSend` over the harness's `start`. */
    send: (imageDataUrl?: string) =>
      machine.beginSend(imageDataUrl, (onEvent, onFailure) =>
        sends.push({ onEvent, onFailure }),
      ),
  };
}

/** The last emitted state, its two blocks, and the Nth Send's callbacks. */
const last = (h: ReturnType<typeof makeMachine>): AgentState =>
  h.states.at(-1)!;
const session = (h: ReturnType<typeof makeMachine>) => last(h).session;
const run = (h: ReturnType<typeof makeMachine>) => last(h).run;
const onEvent = (h: ReturnType<typeof makeMachine>, n = 0) =>
  h.sends[n]!.onEvent;
const onFailure = (h: ReturnType<typeof makeMachine>, n = 0) =>
  h.sends[n]!.onFailure;

describe("createAgentMachine session lifecycle", () => {
  it("starts with no session and emits nothing", () => {
    const h = makeMachine();
    expect(h.machine.getState().session.status).toBe("none");
    expect(h.states).toHaveLength(0);
  });

  it("a send without a session is a no-op", () => {
    const h = makeMachine();
    h.send();
    expect(h.sends).toHaveLength(0);
    expect(h.states).toHaveLength(0);
  });

  it("begin shows the session as unused and idle", async () => {
    const h = makeMachine();
    await h.begin();
    expect(session(h).status).toBe("unused");
    expect(run(h).phase).toBe("idle");
  });

  it("a send marks the session used and runs", async () => {
    const h = makeMachine();
    await h.begin();
    h.send();
    expect(session(h).status).toBe("used");
    expect(run(h).phase).toBe("running");
    expect(h.sends).toHaveLength(1);
  });

  it("a second send after a reply runs again", async () => {
    const h = makeMachine();
    await h.begin();
    h.send();
    onEvent(h)({ kind: "sse_done" });
    h.send();
    expect(h.sends).toHaveLength(2);
    expect(run(h).phase).toBe("running");
  });

  it("end returns to none and clears the text", async () => {
    const h = makeMachine();
    await h.begin();
    h.send();
    onEvent(h)({ kind: "content", text: "early" });
    h.machine.end();
    expect(session(h).status).toBe("none");
    expect(run(h).phase).toBe("idle");
    expect(run(h).content).toBe("");
    // The session is gone, so a later send is a no-op.
    h.send();
    expect(h.sends).toHaveLength(1);
  });

  it("a failed begin surfaces a provider failure and leaves no session", async () => {
    const h = makeMachine();
    h.opener.begin = () => Promise.reject(new Error("offline"));
    await h.begin();
    expect(run(h).phase).toBe("failed");
    expect(session(h).status).toBe("none");
    expect(run(h).failure).toEqual({
      kind: "provider",
      error: { kind: "upstream", code: null, message: "offline" },
    });
  });

  it("preserves a ProviderError rejected by begin", async () => {
    const h = makeMachine();
    h.opener.begin = () =>
      Promise.reject({ kind: "config", code: null, message: "no key" });
    await h.begin();
    expect(run(h).failure).toEqual({
      kind: "provider",
      error: { kind: "config", code: null, message: "no key" },
    });
    expect(session(h).status).toBe("none");
  });

  it("ignores a begin superseded by end", async () => {
    const h = makeMachine();
    let resolve!: () => void;
    h.opener.begin = () =>
      new Promise((r) => {
        resolve = r;
      });
    const pending = h.begin();
    h.machine.end();
    resolve();
    await pending;
    expect(session(h).status).toBe("none");
    h.send();
    expect(h.sends).toHaveLength(0);
  });

  it("fills the message list from the snapshot after begin", async () => {
    const h = makeMachine();
    h.opener.read = async () => [{ role: "system", content: "be helpful" }];

    await h.begin();

    expect(session(h).messages).toEqual([
      { role: "system", content: "be helpful" },
    ]);
  });

  it("shows the session before the snapshot lands, then fills it", async () => {
    const h = makeMachine();
    let resolveRead!: (messages: Message[]) => void;
    h.opener.read = () =>
      new Promise((r) => {
        resolveRead = r;
      });

    const pending = h.begin();
    await flush();
    // The box shows the live Session while the read is still in flight.
    expect(session(h).status).toBe("unused");
    expect(session(h).messages).toEqual([]);

    resolveRead([{ role: "system", content: "be helpful" }]);
    await pending;
    expect(session(h).messages).toEqual([
      { role: "system", content: "be helpful" },
    ]);
  });

  it("keeps the message list across a send", async () => {
    const h = makeMachine();
    h.opener.read = async () => [{ role: "system", content: "be helpful" }];
    await h.begin();

    h.send();

    expect(session(h).messages).toEqual([
      { role: "system", content: "be helpful" },
    ]);
  });

  it("ignores a snapshot read superseded by end", async () => {
    const h = makeMachine();
    let resolveRead!: (messages: Message[]) => void;
    const snapshot = new Promise<Message[]>((r) => {
      resolveRead = r;
    });
    h.opener.read = () => snapshot;
    const pending = h.begin();
    await flush(); // the snapshot is in flight now

    h.machine.end();
    resolveRead([{ role: "system", content: "be helpful" }]);
    await pending;

    expect(session(h).status).toBe("none");
    expect(session(h).messages).toEqual([]);
  });

  it("keeps a snapshot whose read straddles a send", async () => {
    const h = makeMachine();
    let resolveRead!: (messages: Message[]) => void;
    h.opener.read = () =>
      new Promise<Message[]>((r) => {
        resolveRead = r;
      });

    const pending = h.begin();
    await flush(); // the snapshot read is in flight
    h.send(); // the player starts a Send before it lands

    resolveRead([{ role: "system", content: "be helpful" }]);
    await pending;
    expect(session(h).messages).toEqual([
      { role: "system", content: "be helpful" },
    ]);
  });

  it("broadcasts a snapshot a listener cannot write back into the machine", async () => {
    const h = makeMachine();
    await h.begin();

    last(h).session.status = "used";

    expect(h.machine.getState().session.status).toBe("unused");
  });
});

describe("createAgentMachine run transitions", () => {
  async function emptySession() {
    const h = makeMachine();
    await h.begin();
    return h;
  }

  it("accumulates reasoning and content streams", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({ kind: "reasoning", text: "a" });
    onEvent(h)({ kind: "content", text: "hi" });
    onEvent(h)({ kind: "reasoning", text: "b" });
    onEvent(h)({ kind: "content", text: " (2,3)" });

    expect(run(h).phase).toBe("running");
    expect(run(h).reasoning).toBe("ab");
    expect(run(h).content).toBe("hi (2,3)");
  });

  it("sse_done completes the run and keeps the session used", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({ kind: "sse_done" });
    expect(run(h).phase).toBe("done");
    expect(session(h).status).toBe("used");
  });

  it("an interrupted send keeps the session used", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({ kind: "interrupted" });
    expect(run(h).phase).toBe("interrupted");
    expect(session(h).status).toBe("used");
  });

  it("a later interrupted send keeps the session used", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({ kind: "sse_done" });
    h.send();
    onEvent(h, 1)({ kind: "interrupted" });
    expect(session(h).status).toBe("used");
  });

  it("a send failure enters failed and keeps the session used", async () => {
    const h = await emptySession();
    h.send();
    onFailure(h)({
      kind: "provider",
      error: { kind: "config", code: null, message: "no" },
    });
    expect(run(h).phase).toBe("failed");
    expect(session(h).status).toBe("used");
    expect(run(h).failure).toEqual({
      kind: "provider",
      error: { kind: "config", code: null, message: "no" },
    });
  });

  it("a mid-stream provider_error event fails with the provider failure", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({
      kind: "provider_error",
      error: { kind: "upstream", code: 429, message: "rate limited" },
    });
    expect(run(h).phase).toBe("failed");
    expect(run(h).failure).toEqual({
      kind: "provider",
      error: { kind: "upstream", code: 429, message: "rate limited" },
    });
  });

  it("a refused failure enters failed with the refusal", async () => {
    const h = await emptySession();
    h.send();
    onFailure(h)({ kind: "refused", status: 409, message: "busy" });
    expect(run(h).phase).toBe("failed");
    expect(run(h).failure).toEqual({
      kind: "refused",
      status: 409,
      message: "busy",
    });
  });

  it("end invalidates a stale in-flight stream", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({ kind: "content", text: "early" });
    h.machine.end();
    expect(run(h).phase).toBe("idle");
    expect(run(h).content).toBe("");

    // A late event from the superseded run must not corrupt the state.
    onEvent(h)({ kind: "content", text: "late" });
    expect(run(h).phase).toBe("idle");
    expect(run(h).content).toBe("");
    onFailure(h)({
      kind: "provider",
      error: { kind: "upstream", code: 500, message: "x" },
    });
    expect(run(h).phase).toBe("idle");
  });

  it("captures the user message event into the run", async () => {
    const h = await emptySession();
    h.send();
    onEvent(h)({ kind: "user", text: "........." });
    expect(run(h).user).toBe(".........");
  });

  it("seeds userImageUrl from the screenshot for the image mode", async () => {
    const h = await emptySession();
    h.send("data:image/png;base64,AAAA");
    expect(run(h).userImageUrl).toBe("data:image/png;base64,AAAA");
    expect(run(h).user).toBe("");
  });

  it("leaves userImageUrl unset for a text mode", async () => {
    const h = await emptySession();
    h.send();
    expect(run(h).userImageUrl).toBeUndefined();
  });

  it("onState unsubscribe stops notifications", async () => {
    const h = makeMachine();
    h.unsubscribe();
    await h.begin();
    expect(h.states).toHaveLength(0);
  });
});
