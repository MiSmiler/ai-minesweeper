// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountMode, renderModeSwitcher, type AppDeps } from "./mode";
import { makeGameSnapshot } from "../infra/testUtils";

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

function makeDeps(): AppDeps {
  return {
    getPlayMode: () => "single",
    aiApi: {
      startGuide: vi.fn(),
      interrupt_by_user: vi.fn().mockResolvedValue(undefined),
    },
    captureBoardImage: vi.fn().mockResolvedValue("data:image/png;base64,xxx"),
  };
}

function mount(): HTMLElement {
  const root = document.createElement("div");
  document.body.appendChild(root);
  return root;
}

beforeEach(() => {
  document.body.innerHTML = "";
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("mountMode", () => {
  it("mounts the single composition for 'single'", () => {
    mockFetch();
    const root = mount();
    const dispose = mountMode("single", root, makeDeps());
    expect(root.querySelector(".game-area")).toBeTruthy();
    expect(root.querySelector(".game-top-bar")).toBeTruthy();
    dispose();
  });

  it("mounts the guide composition for 'ai-guide'", () => {
    mockFetch();
    const root = mount();
    const dispose = mountMode("ai-guide", root, makeDeps());
    expect(root.querySelector(".guide-layout")).toBeTruthy();
    expect(root.querySelector(".guide-dashboard")).toBeTruthy();
    expect(root.querySelector(".guide-dialog")).toBeTruthy();
    dispose();
  });

  it("returns a teardown that clears the mounted composition", () => {
    mockFetch();
    const root = mount();
    const disposeSingle = mountMode("single", root, makeDeps());
    disposeSingle();
    expect(root.querySelector(".game-area")).toBeNull();
  });
});

describe("renderModeSwitcher", () => {
  it("renders SinglePlay and AiGuide and marks the current active", () => {
    const root = document.createElement("div");
    renderModeSwitcher(root, "single", vi.fn());
    const buttons = root.querySelectorAll<HTMLButtonElement>(".mode-btn");
    expect(buttons).toHaveLength(2);
    expect(buttons[0].textContent).toBe("SinglePlay");
    expect(buttons[1].textContent).toBe("AiGuide");
    expect(buttons[0].classList.contains("active")).toBe(true);
    expect(buttons[1].classList.contains("active")).toBe(false);
  });

  it("calls onSwitch with the clicked mode", () => {
    const root = document.createElement("div");
    const onSwitch = vi.fn();
    renderModeSwitcher(root, "single", onSwitch);
    const buttons = root.querySelectorAll<HTMLButtonElement>(".mode-btn");
    buttons[1].click();
    expect(onSwitch).toHaveBeenCalledWith("ai-guide");
  });

  it("re-renders the active highlight when called with a new current", () => {
    const root = document.createElement("div");
    renderModeSwitcher(root, "single", vi.fn());
    renderModeSwitcher(root, "ai-guide", vi.fn());
    const buttons = root.querySelectorAll<HTMLButtonElement>(".mode-btn");
    expect(buttons[0].classList.contains("active")).toBe(false);
    expect(buttons[1].classList.contains("active")).toBe(true);
  });
});
