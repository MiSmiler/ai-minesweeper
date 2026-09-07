// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createGameArea } from "./gameArea";
import { makeGameSnapshot } from "../infra/testUtils";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/** Stubs the global fetch so the real postAction/fetchSnapshot adapters
 * resolve against a fixed snapshot. */
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

describe("createGameArea", () => {
  it("builds the game area DOM", () => {
    mockFetch();
    const root = mount();
    const area = createGameArea(root);
    const container = root.querySelector(".game-area")!;
    expect(container).toBeTruthy();
    expect(container.querySelector(".difficulty-row")).toBeTruthy();
    expect(container.querySelector(".game-frame")).toBeTruthy();
    expect(container.querySelector(".game-top-bar")).toBeTruthy();
    expect(container.querySelector(".board-host")).toBeTruthy();
    area.dispose();
  });

  it("renders the board on init", async () => {
    mockFetch();
    const root = mount();
    const area = createGameArea(root);
    await flush();
    expect(area.boardEl.querySelectorAll(".cell").length).toBeGreaterThan(0);
    area.dispose();
  });

  it("hands the client a working set of top-bar elements", async () => {
    mockFetch(makeGameSnapshot({ flags_remaining: 10, elapsed_secs: 7 }));
    const root = mount();
    const area = createGameArea(root);
    await flush();
    expect(area.topBarEls.counter.textContent).toBe("010");
    expect(area.topBarEls.timer.textContent).toBe("007");
    area.dispose();
  });

  it("calls onNewGame when a new-game action confirms", async () => {
    mockFetch();
    const root = mount();
    const onNewGame = vi.fn();
    const area = createGameArea(root, { onNewGame });
    await flush();
    area.client.newGame("expert");
    await flush();
    expect(onNewGame).toHaveBeenCalledTimes(1);
    area.dispose();
  });

  it("dispose removes the container", () => {
    mockFetch();
    const root = mount();
    const area = createGameArea(root);
    area.dispose();
    expect(root.querySelector(".game-area")).toBeNull();
  });
});
