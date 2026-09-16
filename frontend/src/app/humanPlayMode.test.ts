// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { composeHumanPlayMode } from "./humanPlayMode";
import type { AppDeps } from "./mode";
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

beforeEach(() => {
  document.body.innerHTML = "";
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("composeHumanPlayMode", () => {
  it("mounts a single game area", () => {
    mockFetch();
    const root = document.createElement("div");
    document.body.appendChild(root);
    const { dispose } = composeHumanPlayMode(root, makeDeps());
    expect(root.querySelector(".game-area")).toBeTruthy();
    expect(root.querySelector(".game-top-bar")).toBeTruthy();
    expect(root.querySelector(".difficulty-row")).toBeTruthy();
    dispose();
  });

  it("dispose removes the mounted composition", () => {
    mockFetch();
    const root = document.createElement("div");
    document.body.appendChild(root);
    const { dispose } = composeHumanPlayMode(root, makeDeps());
    dispose();
    expect(root.querySelector(".game-area")).toBeNull();
  });
});
