// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { startGamePage } from "./gamePage";
import { wirePageNav, type PageNavDeps } from "./pageNav";
import type { TopBarEls } from "./render";
import { gameState, mockBoardGeometry } from "./testUtils";

/** Mounts the ai page's frame (board + AI panel placeholder, the structure
 * from ai.html) and boots it like aiMain.ts does. */
function mountAiPage() {
  document.body.innerHTML = `
    <div id="app">
      <nav class="page-nav" aria-label="Pages">
        <a class="page-nav-link" data-nav href="/">Human</a>
        <span class="page-nav-link active" aria-current="page">AI</span>
      </nav>
      <div class="difficulty-row">
        <button class="difficulty" data-difficulty="beginner">Beginner</button>
        <button class="difficulty" data-difficulty="intermediate">Intermediate</button>
        <button class="difficulty" data-difficulty="expert">Expert</button>
      </div>
      <div class="game-frame">
        <div class="top-bar">
          <div id="counter" class="led"></div>
          <button id="smiley" class="smiley">🙂</button>
          <div id="timer" class="led"></div>
        </div>
        <div id="board"></div>
      </div>
      <section id="ai-panel" class="ai-panel" aria-label="AI assistant">
        <h2 class="ai-panel-title">AI Assistant</h2>
        <textarea id="ai-transcript" class="ai-transcript" readonly placeholder="The AI's Board analysis will appear here."></textarea>
      </section>
    </div>
  `;
  const boardEl = document.getElementById("board")!;
  const topBarEls: TopBarEls = {
    counter: document.getElementById("counter")!,
    smiley: document.getElementById("smiley")!,
    timer: document.getElementById("timer")!,
    difficultyRow: document.querySelector(".difficulty-row")!,
  };
  const post = vi.fn(() => Promise.resolve(gameState()));
  const fetchState = vi.fn(() => Promise.resolve(gameState()));
  const page = startGamePage({ boardEl, topBarEls, post, fetchState });
  const navDeps: PageNavDeps = { fetchState, post };
  return { page, boardEl, topBarEls, post, fetchState, navDeps };
}

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("ai page", () => {
  it("renders an interactive Board alongside the AI panel placeholder", async () => {
    const { page, boardEl, post } = mountAiPage();
    await page.start();

    // The re-used bootstrap renders the Board into the ai frame.
    expect(boardEl.querySelectorAll(".cell").length).toBe(4);
    // The AI panel placeholder is present: a read-only white transcript box.
    const panel = document.getElementById("ai-panel")!;
    expect(panel).not.toBeNull();
    expect(panel.classList).toContain("ai-panel");
    const transcript = document.getElementById(
      "ai-transcript",
    ) as HTMLTextAreaElement;
    expect(transcript).not.toBeNull();
    expect(transcript.readOnly).toBe(true);

    // The Board is interactive in the ai frame.
    mockBoardGeometry(boardEl);
    boardEl.dispatchEvent(
      new MouseEvent("mousedown", {
        button: 0,
        clientX: 5,
        clientY: 5,
        bubbles: true,
        cancelable: true,
      }),
    );
    window.dispatchEvent(
      new MouseEvent("mouseup", { button: 0, bubbles: true, cancelable: true }),
    );
    expect(post).toHaveBeenCalledWith({ type: "reveal", row: 0, col: 0 });
  });

  it("wires the Human navigation link to start a fresh Game before navigating", async () => {
    const { page, navDeps, post } = mountAiPage();
    await page.start();

    const navigate = vi.fn();
    wirePageNav(document.getElementById("app")!, navDeps, navigate);
    document
      .querySelector<HTMLAnchorElement>('a[data-nav][href="/"]')!
      .dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true }),
      );
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(post).toHaveBeenCalledWith({
      type: "new-game",
      difficulty: "beginner",
    });
    expect(navigate).toHaveBeenCalledWith("/");
  });
});
