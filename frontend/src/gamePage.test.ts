// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GameSnapshot } from "./api";
import { startGamePage } from "./gamePage";
import type { TopBarEls } from "./render";
import { CELL, PITCH, gameState, mockBoardGeometry } from "./testUtils";

/** Mounts a game frame in the document and starts a page over it. */
function startedPage(fetchState?: () => Promise<GameSnapshot>) {
  document.body.innerHTML = `
    <div class="difficulty-row">
      <button data-difficulty="beginner">Beginner</button>
      <button data-difficulty="intermediate">Intermediate</button>
      <button data-difficulty="expert">Expert</button>
    </div>
    <div class="top-bar">
      <div id="counter" class="led"></div>
      <button id="smiley" class="smiley">🙂</button>
      <div id="timer" class="led"></div>
    </div>
    <div id="board"></div>
  `;
  const boardEl = document.getElementById("board")!;
  const topBarEls: TopBarEls = {
    counter: document.getElementById("counter")!,
    smiley: document.getElementById("smiley")!,
    timer: document.getElementById("timer")!,
    difficultyRow: document.querySelector(".difficulty-row")!,
  };
  const post = vi.fn(() => Promise.resolve(gameState()));
  const fetchStateMock = vi.fn(
    fetchState ?? (() => Promise.resolve(gameState())),
  );
  const page = startGamePage({
    boardEl,
    topBarEls,
    post,
    fetchState: fetchStateMock,
  });
  return { page, boardEl, topBarEls, post, fetchState: fetchStateMock };
}

const leftPress = (el: HTMLElement, x: number, y: number): void => {
  el.dispatchEvent(
    new MouseEvent("mousedown", {
      button: 0,
      clientX: x,
      clientY: y,
      bubbles: true,
      cancelable: true,
    }),
  );
};

const rightPress = (el: HTMLElement, x: number, y: number): void => {
  el.dispatchEvent(
    new MouseEvent("mousedown", {
      button: 2,
      clientX: x,
      clientY: y,
      bubbles: true,
      cancelable: true,
    }),
  );
};

const releaseOnWindow = (button: number): void => {
  window.dispatchEvent(
    new MouseEvent("mouseup", { button, bubbles: true, cancelable: true }),
  );
};

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("startGamePage", () => {
  it("reveals the Cell under a left press and release", async () => {
    const { page, boardEl, post } = startedPage();
    await page.start();
    mockBoardGeometry(boardEl);

    leftPress(boardEl, 5, 5);
    releaseOnWindow(0);

    expect(post).toHaveBeenCalledWith({ type: "reveal", row: 0, col: 0 });
  });

  it("flags the Cell under a right press", async () => {
    const { page, boardEl, post } = startedPage();
    await page.start();
    mockBoardGeometry(boardEl);

    rightPress(boardEl, PITCH + CELL / 2, PITCH + CELL / 2);

    expect(post).toHaveBeenCalledWith({ type: "flag", row: 1, col: 1 });
  });

  it("a difficulty button starts a fresh game of that difficulty", async () => {
    const { page, topBarEls, post } = startedPage();
    await page.start();

    const expert = topBarEls.difficultyRow.querySelector<HTMLElement>(
      '[data-difficulty="expert"]',
    )!;
    expert.dispatchEvent(new MouseEvent("click", { bubbles: true }));

    expect(post).toHaveBeenCalledWith({
      type: "new-game",
      difficulty: "expert",
    });
  });

  it("the Smiley restarts with the current difficulty", async () => {
    const { page, topBarEls, post } = startedPage();
    await page.start();

    topBarEls.smiley.dispatchEvent(new MouseEvent("click", { bubbles: true }));

    expect(post).toHaveBeenCalledWith({ type: "new-game" });
  });

  it("shows a load failure in the Board", async () => {
    const { page, boardEl } = startedPage(() =>
      Promise.reject(new Error("boom")),
    );

    await page.start();

    expect(boardEl.textContent).toBe("Failed to load game: boom");
  });

  it("is reusable by a second page's game frame", async () => {
    // A first page boots and renders.
    const first = startedPage();
    await first.page.start();
    expect(first.boardEl.querySelectorAll(".cell").length).toBe(4);

    // A second, independent page with its own frame and ids.
    document.body.innerHTML = `
      <div id="ai-page">
        <div class="difficulty-row">
          <button data-difficulty="expert">Expert</button>
        </div>
        <div class="top-bar">
          <div id="ai-counter" class="led"></div>
          <button id="ai-smiley" class="smiley">🙂</button>
          <div id="ai-timer" class="led"></div>
        </div>
        <div id="ai-board"></div>
      </div>
    `;
    const board2 = document.getElementById("ai-board")!;
    const topBar2: TopBarEls = {
      counter: document.getElementById("ai-counter")!,
      smiley: document.getElementById("ai-smiley")!,
      timer: document.getElementById("ai-timer")!,
      difficultyRow: document.querySelector("#ai-page .difficulty-row")!,
    };
    const post2 = vi.fn(() => Promise.resolve(gameState()));
    const page2 = startGamePage({
      boardEl: board2,
      topBarEls: topBar2,
      post: post2,
      fetchState: () => Promise.resolve(gameState()),
    });
    await page2.start();

    // The second page renders and responds to its own inputs.
    expect(board2.querySelectorAll(".cell").length).toBe(4);
    mockBoardGeometry(board2);
    leftPress(board2, 5, 5);
    releaseOnWindow(0);
    expect(post2).toHaveBeenCalledWith({ type: "reveal", row: 0, col: 0 });

    // Its own difficulty row is wired, independent of the first page's.
    topBar2.difficultyRow
      .querySelector<HTMLElement>('[data-difficulty="expert"]')!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(post2).toHaveBeenCalledWith({
      type: "new-game",
      difficulty: "expert",
    });
  });
});
