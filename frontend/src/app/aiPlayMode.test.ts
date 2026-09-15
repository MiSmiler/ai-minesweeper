// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiApi, ReplyEvent, ProviderError } from "../ai-player/api";
import { composeAiPlayMode } from "./aiPlayMode";
import type { AppDeps } from "./mode";
import { makeGameSnapshot } from "../game/testUtils";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/** Stubs the global fetch so the game area's client loads against a snapshot. */
function mockFetch(snapshot = makeGameSnapshot()): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async () =>
        ({
          ok: true,
          json: async () => snapshot,
        }) as Response,
    ),
  );
}

interface AiPlayHarness {
  deps: AppDeps;
  aiApi: AiApi & {
    createSession: ReturnType<typeof vi.fn>;
    send: ReturnType<typeof vi.fn>;
  };
  onEventCalls: Array<(e: ReplyEvent) => void>;
  onProviderErrorCalls: Array<(e: ProviderError) => void>;
}

function makeHarness(): AiPlayHarness {
  const onEventCalls: Array<(e: ReplyEvent) => void> = [];
  const onProviderErrorCalls: Array<(e: ProviderError) => void> = [];
  let seq = 0;
  const aiApi = {
    createSession: vi.fn(async () => ({ sessionId: `session-${seq++}` })),
    send: vi.fn((_sid, _req, onEvent, onProviderError) => {
      onEventCalls.push(onEvent);
      onProviderErrorCalls.push(onProviderError);
    }),
    interrupt_by_user: vi.fn().mockResolvedValue(undefined),
  } as AiApi & {
    createSession: ReturnType<typeof vi.fn>;
    send: ReturnType<typeof vi.fn>;
  };
  const deps: AppDeps = {
    getPlayMode: () => "ai",
    aiApi,
    captureBoardImage: vi.fn().mockResolvedValue("data:image/png;base64,xxx"),
  };
  return { deps, aiApi, onEventCalls, onProviderErrorCalls };
}

function mount(): HTMLElement {
  const root = document.createElement("div");
  document.body.appendChild(root);
  return root;
}

function $(root: HTMLElement, sel: string): HTMLElement {
  return root.querySelector(sel)!;
}

/** Clicks the new-session button and waits for the backend round trip. */
async function startSession(root: HTMLElement): Promise<void> {
  root.querySelector<HTMLButtonElement>(".new-session-btn")!.click();
  await flush();
}

/** Creates a session and commits one Send so the session is non-empty. */
async function seedOneHistoryEntry(
  root: HTMLElement,
  h: AiPlayHarness,
): Promise<void> {
  await startSession(root);
  root.querySelector<HTMLButtonElement>(".send-btn")!.click();
  h.onEventCalls[0]!({ kind: "content", text: "analysis result" });
  h.onEventCalls[0]!({ kind: "sse_done" });
  await flush();
}

beforeEach(() => {
  document.body.innerHTML = "";
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("composeAiPlayMode layout", () => {
  it("builds the three zones", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const layout = root.querySelector(".ai-play-layout")!;
    expect(layout.querySelector(".ai-play-game")).toBeTruthy();
    expect(layout.querySelector(".ai-play-dashboard")).toBeTruthy();
    expect(layout.querySelector(".ai-play-dialog")).toBeTruthy();
    // The game zone holds a fully independent game area.
    expect(layout.querySelector(".ai-play-game .game-area")).toBeTruthy();
    expect(layout.querySelector(".ai-play-game .game-top-bar")).toBeTruthy();
  });

  it("dashboard has send, new session, input mode, axis, history — no strategy", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const dash = $(root, ".ai-play-dashboard");
    expect(dash.querySelector(".send-btn")).toBeTruthy();
    expect(dash.querySelector(".new-session-btn")).toBeTruthy();
    expect(dash.querySelector(".input-mode-select")).toBeTruthy();
    expect(dash.querySelector(".axis-checkbox")).toBeTruthy();
    expect(dash.querySelector(".history-list")).toBeTruthy();
    // The SessionStrategy dropdown is gone (issue #125).
    expect(dash.querySelector(".strategy-select")).toBeNull();
    expect(dash.textContent).not.toContain("会话策略");
  });

  it("places the new-session button to the left of Send", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const row = $(root, ".button-row");
    expect(row.children[0]?.classList.contains("new-session-btn")).toBe(true);
    expect(row.children[1]?.classList.contains("send-btn")).toBe(true);
  });

  it("input mode select offers all three modes", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const opts = $(root, ".input-mode-select").querySelectorAll("option");
    expect(opts).toHaveLength(3);
    expect(Array.from(opts).map((o) => o.value)).toEqual([
      "plain",
      "emoji",
      "image",
    ]);
  });

  it("level select offers the four levels, default low", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const select = $(root, ".level-select") as HTMLSelectElement;
    const opts = select.querySelectorAll("option");
    expect(opts).toHaveLength(4);
    expect(Array.from(opts).map((o) => o.value)).toEqual([
      "off",
      "low",
      "high",
      "max",
    ]);
    expect(select.value).toBe("low");
  });

  it("history starts empty", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    expect($(root, ".history-empty")).toBeTruthy();
    expect(
      $(root, ".history-list").querySelectorAll(".history-entry"),
    ).toHaveLength(0);
  });

  it("the axis layer is hidden by default", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const layer = root.querySelector(".axis-label-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("renders 0-based row/col labels for the loaded board (issue #118)", async () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    await flush(); // let the board load, which fires onRender → setRowsCols
    const layer = root.querySelector(".axis-label-layer")!;
    const rows = layer.querySelectorAll(".axis-row");
    const cols = layer.querySelectorAll(".axis-col");
    expect(rows).toHaveLength(2); // makeGameSnapshot() is 2×2
    expect(cols).toHaveLength(2);
    expect(Array.from(rows).map((e) => e.textContent)).toEqual(["0", "1"]);
    expect(Array.from(cols).map((e) => e.textContent)).toEqual(["0", "1"]);
  });
});

describe("composeAiPlayMode session controls", () => {
  it("disables Send with no session and leaves the InputMode select enabled", () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;
    expect(send.disabled).toBe(true);
    expect(mode.disabled).toBe(false);
  });

  it("a new session enables Send and keeps the InputMode select enabled", async () => {
    mockFetch();
    const root = mount();
    composeAiPlayMode(root, makeHarness().deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;
    expect(send.disabled).toBe(false);
    expect(mode.disabled).toBe(false);
  });

  it("the first committed Send locks the InputMode", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;

    send.click(); // running: locked while the Send is in flight
    expect(mode.disabled).toBe(true);
    expect(send.textContent).toBe("中断");
    h.onEventCalls[0]!({ kind: "sse_done" });
    expect(mode.disabled).toBe(true); // committed: still locked
    expect(send.textContent).toBe("发送");
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);
  });

  it("an interrupted first Send leaves the InputMode editable", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;

    send.click();
    h.onEventCalls[0]!({ kind: "interrupt", reason: "user_interrupt" });
    expect(mode.disabled).toBe(false);
    expect($(root, ".dialog-interrupt").textContent).toContain("已中断");
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
  });

  it("Send is a no-op with no session", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    ($(root, ".send-btn") as HTMLButtonElement).click();
    expect(h.aiApi.send).not.toHaveBeenCalled();
  });
});

describe("composeAiPlayMode send flow", () => {
  it("streams events into the dialog and records history on done", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;

    send.click();
    expect(h.aiApi.send).toHaveBeenCalledTimes(1);
    const req = h.aiApi.send.mock.calls[0][1] as { inputMode: string };
    expect(req.inputMode).toBe("plain");
    expect(h.aiApi.send.mock.calls[0][0]).toBe("session-0");

    h.onEventCalls[0]!({ kind: "reasoning", text: "think" });
    h.onEventCalls[0]!({ kind: "content", text: "(2,3)" });
    expect($(root, ".dialog-reasoning").textContent).toBe("think");
    expect($(root, ".dialog-content").textContent).toBe("(2,3)");

    h.onEventCalls[0]!({ kind: "sse_done" });
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);
  });

  it("interrupt calls interrupt_by_user and the event reverts the button", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    send.click(); // send → interrupt
    expect(h.aiApi.interrupt_by_user).toHaveBeenCalledTimes(1);

    h.onEventCalls[0]!({ kind: "interrupt", reason: "user_interrupt" });
    expect(send.textContent).toBe("发送");
  });

  it("captures a screenshot for the image mode", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const select = root.querySelector<HTMLSelectElement>(".input-mode-select")!;
    select.value = "image";
    select.dispatchEvent(new Event("change"));

    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    await flush();
    expect(h.deps.captureBoardImage).toHaveBeenCalled();
    const req = h.aiApi.send.mock.calls[0][1] as {
      inputMode: string;
      imageDataUrl?: string;
    };
    expect(req.inputMode).toBe("image");
    expect(req.imageDataUrl).toBeTruthy();
  });

  it("sends the selected thinking level and keeps history on change", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;

    send.click();
    let req = h.aiApi.send.mock.calls[0][1] as {
      inputMode: string;
      thinkingLevel: string;
    };
    expect(req.thinkingLevel).toBe("low");
    h.onEventCalls[0]!({ kind: "sse_done" });
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    // Switch the level: no confirm, history kept.
    const level = root.querySelector<HTMLSelectElement>(".level-select")!;
    level.value = "max";
    level.dispatchEvent(new Event("change"));
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    // The next Send carries the new level.
    send.click();
    req = h.aiApi.send.mock.calls[1][1] as {
      inputMode: string;
      thinkingLevel: string;
    };
    expect(req.thinkingLevel).toBe("max");
  });

  it("a provider error alerts and reverts the button", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    h.onProviderErrorCalls[0]!({
      kind: "config",
      code: null,
      message: "no provider",
    });
    expect(alertSpy).toHaveBeenCalled();
    expect(send.textContent).toBe("发送");
  });

  it("streams the user message into the dialog box", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click();
    h.onEventCalls[0]!({ kind: "user", text: "........." });
    expect($(root, ".dialog-user-text").textContent).toBe(".........");
  });

  it("shows an image thumbnail in the dialog for the image form", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    const select = root.querySelector<HTMLSelectElement>(".input-mode-select")!;
    select.value = "image";
    select.dispatchEvent(new Event("change"));
    ($(root, ".send-btn") as HTMLButtonElement).click();
    await flush(); // let captureBoardImage resolve; send seeds userImageUrl
    const img = root.querySelector(".dialog-user-image") as HTMLImageElement;
    expect(img.hidden).toBe(false);
    expect(img.src).toBe("data:image/png;base64,xxx");
  });
});

describe("composeAiPlayMode session lifecycle", () => {
  it("a failed new session alerts and leaves Send disabled", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    h.aiApi.createSession.mockRejectedValueOnce({
      kind: "config",
      code: null,
      message: "no provider",
    });
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    expect(alertSpy).toHaveBeenCalled();
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(true);
  });

  it("a new session clears history after confirmation", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    composeAiPlayMode(root, h.deps);
    await seedOneHistoryEntry(root, h);
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    await startSession(root);
    expect(confirmSpy).toHaveBeenCalled();
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
    expect($(root, ".history-empty")).toBeTruthy();
    expect(h.aiApi.createSession).toHaveBeenCalledTimes(2);
  });

  it("declining a new session keeps history and the current session", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    vi.spyOn(window, "confirm").mockReturnValue(false);
    composeAiPlayMode(root, h.deps);
    await seedOneHistoryEntry(root, h);

    await startSession(root);
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);
    expect(h.aiApi.createSession).toHaveBeenCalledTimes(1);
  });

  it("a new session never confirms while the session is empty", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    confirmSpy.mockClear();
    await startSession(root);
    expect(confirmSpy).not.toHaveBeenCalled();
  });

  it("a new session interrupts an in-flight Send first", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click(); // running

    await startSession(root);
    expect(h.aiApi.interrupt_by_user).toHaveBeenCalledTimes(1);
    expect($(root, ".send-btn") as HTMLButtonElement).toBeTruthy();
  });

  it("a new game clears history after confirmation", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    composeAiPlayMode(root, h.deps);
    await flush(); // init done, so the game area click listener is live
    await seedOneHistoryEntry(root, h);
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    const difficulty = root.querySelector<HTMLButtonElement>(
      '[data-difficulty="beginner"]',
    )!;
    difficulty.click();
    await flush();
    expect(confirmSpy).toHaveBeenCalled();
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
    expect($(root, ".history-empty")).toBeTruthy();
  });

  it("declining a new game keeps history and does not start one", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    composeAiPlayMode(root, h.deps);
    await flush();
    await seedOneHistoryEntry(root, h);

    const difficulty = root.querySelector<HTMLButtonElement>(
      '[data-difficulty="beginner"]',
    )!;
    difficulty.click();
    await flush();
    expect(confirmSpy).toHaveBeenCalled();
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);
    expect($(root, ".history-empty")).toBeFalsy();
  });

  it("the smiley starts a new game after confirmation", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    composeAiPlayMode(root, h.deps);
    await flush();
    await seedOneHistoryEntry(root, h);

    const smiley = root.querySelector<HTMLButtonElement>(".smiley")!;
    smiley.click();
    await flush();
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
    expect($(root, ".history-empty")).toBeTruthy();
  });

  it("a new game on an in-flight first Send interrupts without confirming", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    composeAiPlayMode(root, h.deps);
    await flush();
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click(); // running, still empty

    const smiley = root.querySelector<HTMLButtonElement>(".smiley")!;
    smiley.click();
    await flush();
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(h.aiApi.interrupt_by_user).toHaveBeenCalledTimes(1);
  });

  it("changing the InputMode while empty does not confirm or clear", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    composeAiPlayMode(root, h.deps);
    await startSession(root);
    confirmSpy.mockClear();

    const select = root.querySelector<HTMLSelectElement>(".input-mode-select")!;
    select.value = "emoji";
    select.dispatchEvent(new Event("change"));
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
    expect(select.value).toBe("emoji");
  });

  it("guards refresh / mode switch via hasSessionHistory and confirmDiscard", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const composition = composeAiPlayMode(root, h.deps);
    expect(composition.hasSessionHistory!()).toBe(false);
    // Nothing to discard: confirmDiscard proceeds without asking.
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    expect(composition.confirmDiscard!("msg")).toBe(true);
    expect(confirmSpy).not.toHaveBeenCalled();

    // An empty (created but uncommitted) session does not guard either.
    await startSession(root);
    expect(composition.hasSessionHistory!()).toBe(false);

    await seedOneHistoryEntry(root, h);
    expect(composition.hasSessionHistory!()).toBe(true);
    confirmSpy.mockReturnValue(false);
    expect(composition.confirmDiscard!("msg")).toBe(false);
    expect(confirmSpy).toHaveBeenCalledWith("msg");
    confirmSpy.mockReturnValue(true);
    expect(composition.confirmDiscard!("msg")).toBe(true);
  });

  it("the axis checkbox toggles the axis layer", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeAiPlayMode(root, h.deps);
    const checkbox = root.querySelector<HTMLInputElement>(".axis-checkbox")!;
    const layer = root.querySelector(".axis-label-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
    checkbox.checked = true;
    checkbox.dispatchEvent(new Event("change"));
    expect(layer.classList.contains("hidden")).toBe(false);
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event("change"));
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("dispose tears down the composition", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const { dispose } = composeAiPlayMode(root, h.deps);
    dispose();
    expect(root.querySelector(".ai-play-layout")).toBeNull();
  });
});
