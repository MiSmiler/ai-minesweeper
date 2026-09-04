// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiApi, GuideEvent, ProviderError } from "../ai/api";
import { composeGuideMode } from "./guideMode";
import type { AppDeps } from "./mode";
import { makeGameSnapshot } from "../infra/testUtils";

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

interface GuideHarness {
  deps: AppDeps;
  aiApi: AiApi & { startGuide: ReturnType<typeof vi.fn> };
  onEventCalls: Array<(e: GuideEvent) => void>;
  onProviderErrorCalls: Array<(e: ProviderError) => void>;
}

function makeHarness(): GuideHarness {
  const onEventCalls: Array<(e: GuideEvent) => void> = [];
  const onProviderErrorCalls: Array<(e: ProviderError) => void> = [];
  const aiApi = {
    startGuide: vi.fn((_sid, _req, onEvent, onProviderError) => {
      onEventCalls.push(onEvent);
      onProviderErrorCalls.push(onProviderError);
    }),
    interrupt_by_user: vi.fn().mockResolvedValue(undefined),
  } as AiApi & { startGuide: ReturnType<typeof vi.fn> };
  const deps: AppDeps = {
    getPlayMode: () => "ai-guide",
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

beforeEach(() => {
  document.body.innerHTML = "";
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("composeGuideMode layout", () => {
  it("builds the three zones", () => {
    mockFetch();
    const root = mount();
    composeGuideMode(root, makeHarness().deps);
    const layout = root.querySelector(".guide-layout")!;
    expect(layout.querySelector(".guide-game")).toBeTruthy();
    expect(layout.querySelector(".guide-dashboard")).toBeTruthy();
    expect(layout.querySelector(".guide-dialog")).toBeTruthy();
    // The game zone holds a fully independent game area.
    expect(layout.querySelector(".guide-game .game-area")).toBeTruthy();
    expect(layout.querySelector(".guide-game .game-top-bar")).toBeTruthy();
  });

  it("dashboard has analysis button, format select, strategy select, axis checkbox, history", () => {
    mockFetch();
    const root = mount();
    composeGuideMode(root, makeHarness().deps);
    const dash = $(root, ".guide-dashboard");
    expect(dash.querySelector(".analysis-btn")).toBeTruthy();
    expect(dash.querySelector(".format-select")).toBeTruthy();
    expect(dash.querySelector(".strategy-select")).toBeTruthy();
    expect(dash.querySelector(".axis-checkbox")).toBeTruthy();
    expect(dash.querySelector(".history-list")).toBeTruthy();
  });

  it("format select offers all four forms", () => {
    mockFetch();
    const root = mount();
    composeGuideMode(root, makeHarness().deps);
    const opts = $(root, ".format-select").querySelectorAll("option");
    expect(opts).toHaveLength(4);
    expect(Array.from(opts).map((o) => o.value)).toEqual([
      "simple-text",
      "emoji",
      "full-coordinates",
      "image",
    ]);
  });

  it("strategy select marks per-game as disabled and labeled not implemented", () => {
    mockFetch();
    const root = mount();
    composeGuideMode(root, makeHarness().deps);
    const opts = $(root, ".strategy-select").querySelectorAll("option");
    expect(opts[0].disabled).toBe(false);
    expect(opts[1].disabled).toBe(true);
    expect(opts[1].textContent).toContain("(未实现)");
  });

  it("history starts empty", () => {
    mockFetch();
    const root = mount();
    composeGuideMode(root, makeHarness().deps);
    expect($(root, ".history-empty")).toBeTruthy();
    expect(
      $(root, ".history-list").querySelectorAll(".history-entry"),
    ).toHaveLength(0);
  });

  it("the axis layer is hidden by default", () => {
    mockFetch();
    const root = mount();
    composeGuideMode(root, makeHarness().deps);
    const layer = root.querySelector(".axis-label-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
  });
});

describe("composeGuideMode analysis flow", () => {
  it("the analyze button toggles to interrupt and streams events into the dialog", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeGuideMode(root, h.deps);
    const btn = root.querySelector<HTMLButtonElement>(".analysis-btn")!;
    expect(btn.textContent).toBe("分析");

    btn.click();
    expect(btn.textContent).toBe("中断");
    expect(btn.classList.contains("running")).toBe(true);
    expect(h.aiApi.startGuide).toHaveBeenCalledTimes(1);
    const req = h.aiApi.startGuide.mock.calls[0][1] as { format: string };
    expect(req.format).toBe("simple-text");
    const sid = h.aiApi.startGuide.mock.calls[0][0] as string;
    expect(sid.startsWith("session-")).toBe(true);

    const onEvent = h.onEventCalls[0]!;
    onEvent({ kind: "reasoning", text: "think" });
    onEvent({ kind: "content", text: "SUGGEST null" });
    expect($(root, ".dialog-reasoning").textContent).toBe("think");
    expect($(root, ".dialog-content").textContent).toBe("SUGGEST null");

    onEvent({ kind: "sse_done" });
    expect(btn.textContent).toBe("分析");
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);
  });

  it("interrupt calls interrupt_by_user and the interrupt event reverts the button", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeGuideMode(root, h.deps);
    const btn = root.querySelector<HTMLButtonElement>(".analysis-btn")!;
    btn.click();
    btn.click(); // analyze → interrupt
    expect(h.aiApi.interrupt_by_user).toHaveBeenCalledTimes(1);

    h.onEventCalls[0]!({ kind: "interrupt", reason: "user_interrupt" });
    expect(btn.textContent).toBe("分析");
    expect($(root, ".dialog-interrupt").textContent).toContain("已中断");
  });

  it("captures a screenshot for the image format", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    composeGuideMode(root, h.deps);
    const select = root.querySelector<HTMLSelectElement>(".format-select")!;
    select.value = "image";
    select.dispatchEvent(new Event("change"));

    const btn = root.querySelector<HTMLButtonElement>(".analysis-btn")!;
    btn.click();
    await flush();
    expect(h.deps.captureBoardImage).toHaveBeenCalled();
    const req = h.aiApi.startGuide.mock.calls[0][1] as {
      format: string;
      imageDataUrl?: string;
    };
    expect(req.format).toBe("image");
    expect(req.imageDataUrl).toBeTruthy();
  });

  it("a provider error alerts and reverts the button", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    composeGuideMode(root, h.deps);
    const btn = root.querySelector<HTMLButtonElement>(".analysis-btn")!;
    btn.click();
    h.onProviderErrorCalls[0]!({
      kind: "config",
      code: null,
      message: "no provider",
    });
    expect(alertSpy).toHaveBeenCalled();
    expect(btn.textContent).toBe("分析");
  });
});

describe("composeGuideMode history binding", () => {
  async function seedOneHistoryEntry(
    root: HTMLElement,
    h: GuideHarness,
  ): Promise<void> {
    const btn = root.querySelector<HTMLButtonElement>(".analysis-btn")!;
    btn.click();
    h.onEventCalls[0]!({ kind: "content", text: "analysis result" });
    h.onEventCalls[0]!({ kind: "sse_done" });
    await flush();
  }

  it("confirming a format change clears history", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    composeGuideMode(root, h.deps);
    await seedOneHistoryEntry(root, h);
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    const select = root.querySelector<HTMLSelectElement>(".format-select")!;
    select.value = "emoji";
    select.dispatchEvent(new Event("change"));
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
    expect($(root, ".history-empty")).toBeTruthy();
  });

  it("declining a format change keeps history and reverts the selection", () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    composeGuideMode(root, h.deps);
    const btn = root.querySelector<HTMLButtonElement>(".analysis-btn")!;
    btn.click();
    h.onEventCalls[0]!({ kind: "content", text: "x" });
    h.onEventCalls[0]!({ kind: "sse_done" });
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    const select = root.querySelector<HTMLSelectElement>(".format-select")!;
    select.value = "emoji";
    select.dispatchEvent(new Event("change"));
    expect(confirmSpy).toHaveBeenCalled();
    expect(select.value).toBe("simple-text");
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);
  });

  it("a new game clears history after confirmation", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    composeGuideMode(root, h.deps);
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
    composeGuideMode(root, h.deps);
    await flush();
    await seedOneHistoryEntry(root, h);
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

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
    composeGuideMode(root, h.deps);
    await flush();
    await seedOneHistoryEntry(root, h);
    expect(root.querySelectorAll(".history-entry")).toHaveLength(1);

    const smiley = root.querySelector<HTMLButtonElement>(".smiley")!;
    smiley.click();
    await flush();
    expect(root.querySelectorAll(".history-entry")).toHaveLength(0);
    expect($(root, ".history-empty")).toBeTruthy();
  });

  it("guards refresh / mode switch via hasGuideHistory and confirmDiscard", async () => {
    mockFetch();
    const root = mount();
    const h = makeHarness();
    const composition = composeGuideMode(root, h.deps);
    expect(composition.hasGuideHistory!()).toBe(false);
    // No history to discard: confirmDiscard proceeds without asking.
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    expect(composition.confirmDiscard!("msg")).toBe(true);
    expect(confirmSpy).not.toHaveBeenCalled();

    await seedOneHistoryEntry(root, h);
    expect(composition.hasGuideHistory!()).toBe(true);
    // History present: confirmDiscard asks and honors the choice.
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
    composeGuideMode(root, h.deps);
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
    const { dispose } = composeGuideMode(root, h.deps);
    dispose();
    expect(root.querySelector(".guide-layout")).toBeNull();
  });
});
