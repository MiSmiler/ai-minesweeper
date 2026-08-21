// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import type { CellView, GameState } from "./api";
import {
  formatCounter,
  formatTimer,
  renderBoard,
  renderTopBar,
  type TopBarEls,
} from "./render";
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
    <div class="difficulty-row">
      <button data-difficulty="beginner">Beginner</button>
      <button data-difficulty="intermediate">Intermediate</button>
      <button data-difficulty="expert">Expert</button>
    </div>
    <div class="top-bar">
      <div id="counter" class="led"></div>
      <button id="smiley" class="smiley"></button>
      <div id="timer" class="led"></div>
    </div>
  `;
});

describe("formatTimer", () => {
  it("formats elapsed seconds as a three-digit display capped at 999", () => {
    expect(formatTimer(0)).toBe("000");
    expect(formatTimer(59)).toBe("059");
    expect(formatTimer(60)).toBe("060");
    expect(formatTimer(999)).toBe("999");
    expect(formatTimer(1000)).toBe("999");
    expect(formatTimer(3600)).toBe("999");
  });
});

describe("formatCounter", () => {
  it("pads non-negative Flags Remaining to three digits", () => {
    expect(formatCounter(0)).toBe("000");
    expect(formatCounter(5)).toBe("005");
    expect(formatCounter(99)).toBe("099");
  });

  it("renders negative Flags Remaining as minus sign plus digits", () => {
    expect(formatCounter(-1)).toBe("-1");
    expect(formatCounter(-12)).toBe("-12");
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
  /** The top-bar elements from the beforeEach document body. */
  const els = (): TopBarEls => ({
    counter: document.getElementById("counter")!,
    smiley: document.getElementById("smiley")!,
    timer: document.getElementById("timer")!,
    difficultyRow: document.querySelector(".difficulty-row")!,
  });

  it("renders Flags Remaining as a three-digit counter", () => {
    const bar = els();
    renderTopBar(gameState({ flags_remaining: 5 }), bar);
    expect(bar.counter.textContent).toBe("005");
  });

  it("renders negative Flags Remaining with a minus sign", () => {
    const bar = els();
    renderTopBar(gameState({ flags_remaining: -2 }), bar);
    expect(bar.counter.textContent).toBe("-2");
  });

  it("renders the Timer as three-digit seconds", () => {
    const bar = els();
    renderTopBar(gameState({ elapsed_secs: 65 }), bar);
    expect(bar.timer.textContent).toBe("065");
  });

  it("shows the neutral smiley while Ready or Playing", () => {
    const bar = els();
    renderTopBar(gameState({ game_state: "playing" }), bar);
    expect(bar.smiley.textContent).toBe("🙂");
    renderTopBar(gameState({ game_state: "ready" }), bar);
    expect(bar.smiley.textContent).toBe("🙂");
  });

  it("shows the sunglasses smiley on a Won game", () => {
    const bar = els();
    renderTopBar(gameState({ game_state: "won" }), bar);
    expect(bar.smiley.textContent).toBe("😎");
  });

  it("shows the crying smiley on a Lost game", () => {
    const bar = els();
    renderTopBar(gameState({ game_state: "lost" }), bar);
    expect(bar.smiley.textContent).toBe("😭");
  });

  it("highlights the active difficulty button", () => {
    const bar = els();
    renderTopBar(gameState({ difficulty: "intermediate" }), bar);
    const buttons =
      bar.difficultyRow.querySelectorAll<HTMLElement>("[data-difficulty]");
    const active = Array.from(buttons).filter((b) =>
      b.classList.contains("active"),
    );
    expect(active.map((b) => b.dataset.difficulty)).toEqual(["intermediate"]);
  });
});
