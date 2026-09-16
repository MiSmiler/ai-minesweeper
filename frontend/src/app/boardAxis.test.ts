// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { createBoardAxis } from "./boardAxis";

/** Builds a board view holding a `.board` grid of `rows`×`cols` cells. */
function makeBoard(rows: number, cols: number): HTMLElement {
  const view = document.createElement("div");
  const board = document.createElement("div");
  board.className = "board";
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const cell = document.createElement("div");
      cell.className = "cell";
      cell.dataset.row = String(r);
      cell.dataset.col = String(c);
      board.appendChild(cell);
    }
  }
  view.appendChild(board);
  document.body.appendChild(view);
  return view;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("createBoardAxis", () => {
  it("wraps the board view in a .axis-anchor", () => {
    const view = makeBoard(2, 2);
    createBoardAxis(view);
    expect(view.parentElement!.className).toContain("axis-anchor");
  });

  it("keeps the label layer outside the board view (screenshot-safe)", () => {
    const view = makeBoard(2, 2);
    createBoardAxis(view);
    const anchor = view.parentElement!;
    expect(anchor.querySelector(".axis-layer")).toBeTruthy();
    expect(view.querySelector(".axis-layer")).toBeNull();
  });

  it("starts hidden by default", () => {
    const view = makeBoard(2, 2);
    createBoardAxis(view);
    const layer = view.parentElement!.querySelector(".axis-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("starts visible when opts.visible is true", () => {
    const view = makeBoard(2, 2);
    createBoardAxis(view, { visible: true });
    const layer = view.parentElement!.querySelector(".axis-layer")!;
    expect(layer.classList.contains("hidden")).toBe(false);
  });

  it("setVisible toggles the layer", () => {
    const view = makeBoard(2, 2);
    const axis = createBoardAxis(view);
    const layer = view.parentElement!.querySelector(".axis-layer")!;
    axis.setVisible(true);
    expect(layer.classList.contains("hidden")).toBe(false);
    axis.setVisible(false);
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("setRowsCols renders 0-based row labels along the left edge", () => {
    const view = makeBoard(3, 4);
    const axis = createBoardAxis(view);
    axis.setRowsCols(3, 4);
    const rows = view.parentElement!.querySelectorAll<HTMLElement>(".axis-row");
    expect(rows).toHaveLength(3);
    expect(Array.from(rows).map((el) => el.textContent)).toEqual([
      "0",
      "1",
      "2",
    ]);
    expect(Array.from(rows).map((el) => el.dataset.row)).toEqual([
      "0",
      "1",
      "2",
    ]);
  });

  it("setRowsCols renders 0-based col labels along the bottom edge", () => {
    const view = makeBoard(3, 4);
    const axis = createBoardAxis(view);
    axis.setRowsCols(3, 4);
    const cols = view.parentElement!.querySelectorAll<HTMLElement>(".axis-col");
    expect(cols).toHaveLength(4);
    expect(Array.from(cols).map((el) => el.textContent)).toEqual([
      "0",
      "1",
      "2",
      "3",
    ]);
    expect(Array.from(cols).map((el) => el.dataset.col)).toEqual([
      "0",
      "1",
      "2",
      "3",
    ]);
  });

  it("setRowsCols lives inside the label layer, outside the board", () => {
    const view = makeBoard(2, 2);
    const axis = createBoardAxis(view);
    axis.setRowsCols(2, 2);
    const anchor = view.parentElement!;
    expect(anchor.querySelectorAll(".axis-layer .axis-row")).toHaveLength(2);
    expect(anchor.querySelectorAll(".axis-layer .axis-col")).toHaveLength(2);
    // Never inside the board view (screenshot-safe).
    expect(view.querySelector(".axis-row")).toBeNull();
    expect(view.querySelector(".axis-col")).toBeNull();
  });

  it("setRowsCols re-renders on resize without stale labels", () => {
    const view = makeBoard(2, 2);
    const axis = createBoardAxis(view);
    axis.setRowsCols(2, 2);
    expect(view.parentElement!.querySelectorAll(".axis-row")).toHaveLength(2);
    expect(view.parentElement!.querySelectorAll(".axis-col")).toHaveLength(2);

    axis.setRowsCols(3, 5);
    expect(view.parentElement!.querySelectorAll(".axis-row")).toHaveLength(3);
    expect(view.parentElement!.querySelectorAll(".axis-col")).toHaveLength(5);
    expect(
      view.parentElement!.querySelectorAll(".axis-col")[4]!.textContent,
    ).toBe("4");
  });

  it("label visibility follows setVisible", () => {
    const view = makeBoard(2, 2);
    const axis = createBoardAxis(view);
    axis.setRowsCols(2, 2);
    const layer =
      view.parentElement!.querySelector<HTMLElement>(".axis-layer")!;
    axis.setVisible(true);
    expect(layer.classList.contains("hidden")).toBe(false);
    axis.setVisible(false);
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("destroy removes the axis wrapper", () => {
    const view = makeBoard(2, 2);
    const axis = createBoardAxis(view);
    const anchor = view.parentElement!;
    axis.destroy();
    expect(anchor.isConnected).toBe(false);
  });
});
