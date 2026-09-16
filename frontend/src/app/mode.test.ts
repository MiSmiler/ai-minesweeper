// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountMode, renderModeSwitcher, type AppDeps } from "./mode";
import { makeGameSnapshot } from "../game/testUtils";

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
    getPlayMode: () => "human",
    aiApi: {
      begin: vi.fn(async () => {}),
      send: vi.fn(),
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
  it("mounts the HumanPlay composition for 'human'", () => {
    mockFetch();
    const root = mount();
    const composition = mountMode("human", root, makeDeps());
    expect(root.querySelector(".game-area")).toBeTruthy();
    expect(root.querySelector(".game-top-bar")).toBeTruthy();
    composition.dispose();
  });

  it("mounts the AiPlay composition for 'ai'", () => {
    mockFetch();
    const root = mount();
    const composition = mountMode("ai", root, makeDeps());
    expect(root.querySelector(".ai-play-layout")).toBeTruthy();
    expect(root.querySelector(".ai-play-dashboard")).toBeTruthy();
    expect(root.querySelector(".ai-play-session-box")).toBeTruthy();
    composition.dispose();
  });

  it("returns a teardown that clears the mounted composition", () => {
    mockFetch();
    const root = mount();
    const composition = mountMode("human", root, makeDeps());
    composition.dispose();
    expect(root.querySelector(".game-area")).toBeNull();
  });
});

describe("renderModeSwitcher", () => {
  it("renders Human and AI and marks the current active", () => {
    const root = document.createElement("div");
    renderModeSwitcher(root, "human", vi.fn());
    const buttons = root.querySelectorAll<HTMLButtonElement>(".mode-btn");
    expect(buttons).toHaveLength(2);
    expect(buttons[0].textContent).toBe("Human");
    expect(buttons[1].textContent).toBe("AI");
    expect(buttons[0].classList.contains("active")).toBe(true);
    expect(buttons[1].classList.contains("active")).toBe(false);
  });

  it("calls onSwitch with the clicked mode", () => {
    const root = document.createElement("div");
    const onSwitch = vi.fn();
    renderModeSwitcher(root, "human", onSwitch);
    const buttons = root.querySelectorAll<HTMLButtonElement>(".mode-btn");
    buttons[1].click();
    expect(onSwitch).toHaveBeenCalledWith("ai");
  });

  it("re-renders the active highlight when called with a new current", () => {
    const root = document.createElement("div");
    renderModeSwitcher(root, "human", vi.fn());
    renderModeSwitcher(root, "ai", vi.fn());
    const buttons = root.querySelectorAll<HTMLButtonElement>(".mode-btn");
    expect(buttons[0].classList.contains("active")).toBe(false);
    expect(buttons[1].classList.contains("active")).toBe(true);
  });
});
