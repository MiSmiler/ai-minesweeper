// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import type { CellView, GameState } from "./api";
import { formatTimer, renderBoard, renderTopBar } from "./render";
import { gameState } from "./testUtils";

const cell = (
  state: CellView["state"],
  content: CellView["content"],
): CellView => ({
  state,
  content,
});

/** Renders a board into a fresh container and returns its .cell elements. */
function renderCells(state: GameState): HTMLElement[] {
  const container = document.createElement("div");
  document.body.appendChild(container);
  renderBoard(state, container);
  return Array.from(container.querySelectorAll<HTMLElement>(".cell"));
}

beforeEach(() => {
  document.body.innerHTML = `
    <div class="top-bar">
      <span id="counter"></span>
      <span id="banner"></span>
      <span id="timer"></span>
      <button data-difficulty="beginner">Beginner</button>
      <button data-difficulty="intermediate">Intermediate</button>
      <button data-difficulty="expert">Expert</button>
      <button id="new-game">New Game</button>
    </div>
  `;
});

describe("formatTimer", () => {
  it("formats seconds as MM:SS", () => {
    expect(formatTimer(0)).toBe("00:00");
    expect(formatTimer(59)).toBe("00:59");
    expect(formatTimer(60)).toBe("01:00");
    expect(formatTimer(3599)).toBe("59:59");
    expect(formatTimer(3600)).toBe("60:00");
  });
});

describe("renderBoard", () => {
  it("renders one Cell per cell, in row-major order, addressed by row/col", () => {
    const cells = renderCells(
      gameState({
        rows: 2,
        cols: 2,
        cells: [
          cell("hidden", null),
          cell("hidden", null),
          cell("hidden", null),
          cell("hidden", null),
        ],
      }),
    );
    expect(cells).toHaveLength(4);
    expect(cells.map((el) => `${el.dataset.row},${el.dataset.col}`)).toEqual([
      "0,0",
      "0,1",
      "1,0",
      "1,1",
    ]);
  });

  it("sizes the Board to its column count", () => {
    const container = document.createElement("div");
    renderBoard(gameState({ rows: 3, cols: 4, cells: [] }), container);
    const board = container.firstElementChild as HTMLElement;
    expect(board.style.gridTemplateColumns).toBe("repeat(4, var(--cell-size))");
  });

  it("leaves a Hidden Cell empty", () => {
    const [el] = renderCells(gameState({ cells: [cell("hidden", null)] }));
    expect(el.textContent).toBe("");
    expect(el.classList.contains("cell-flagged")).toBe(false);
    expect(el.classList.contains("cell-revealed")).toBe(false);
  });

  it("renders a Flagged Cell with the flag marker", () => {
    const [el] = renderCells(gameState({ cells: [cell("flagged", null)] }));
    expect(el.textContent).toBe("🚩");
    expect(el.classList.contains("cell-flagged")).toBe(true);
  });

  it("renders a Revealed numeric Cell with its number and n-class", () => {
    const [el] = renderCells(gameState({ cells: [cell("revealed", 3)] }));
    expect(el.textContent).toBe("3");
    expect(el.classList.contains("cell-revealed")).toBe(true);
    expect(el.classList.contains("n3")).toBe(true);
  });

  it("renders a Revealed zero Cell empty, without an n-class", () => {
    const [el] = renderCells(gameState({ cells: [cell("revealed", 0)] }));
    expect(el.textContent).toBe("");
    expect(el.classList.contains("cell-revealed")).toBe(true);
    expect(el.classList.contains("n0")).toBe(false);
  });

  it("renders a Revealed Mine with the mine class, and highlights the Trigger Mine", () => {
    const els = renderCells(
      gameState({
        rows: 1,
        cols: 2,
        trigger: { row: 0, col: 1 },
        cells: [cell("revealed", "mine"), cell("revealed", "mine")],
      }),
    );
    expect(els[0].textContent).toBe("💣");
    expect(els[0].classList.contains("cell-mine")).toBe(true);
    expect(els[0].classList.contains("cell-trigger")).toBe(false);
    expect(els[1].classList.contains("cell-trigger")).toBe(true);
  });
});

describe("renderTopBar", () => {
  it("renders the flag counter from flags_remaining, negative when over-flagged", () => {
    renderTopBar(gameState({ flags_remaining: -2 }));
    expect(document.getElementById("counter")!.textContent).toBe("🚩 -2");
  });

  it("renders the WON banner for a Won game", () => {
    renderTopBar(gameState({ game_state: "won" }));
    const banner = document.getElementById("banner")!;
    expect(banner.textContent).toBe("WON");
    expect(banner.classList.contains("won")).toBe(true);
  });

  it("renders the LOST banner for a Lost game", () => {
    renderTopBar(gameState({ game_state: "lost" }));
    const banner = document.getElementById("banner")!;
    expect(banner.textContent).toBe("LOST");
    expect(banner.classList.contains("lost")).toBe(true);
  });

  it("clears the banner while the game is in progress", () => {
    renderTopBar(gameState({ game_state: "playing" }));
    const banner = document.getElementById("banner")!;
    expect(banner.textContent).toBe("");
    expect(banner.classList.contains("won")).toBe(false);
    expect(banner.classList.contains("lost")).toBe(false);
  });

  it("highlights the active difficulty button", () => {
    renderTopBar(gameState({ difficulty: "intermediate" }));
    const buttons = document.querySelectorAll<HTMLElement>("[data-difficulty]");
    const active = Array.from(buttons).filter((b) =>
      b.classList.contains("active"),
    );
    expect(active.map((b) => b.dataset.difficulty)).toEqual(["intermediate"]);
  });

  it("renders the timer from elapsed seconds", () => {
    renderTopBar(gameState({ elapsed_secs: 65 }));
    expect(document.getElementById("timer")!.textContent).toBe("01:05");
  });
});
