// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { createBoardAxis } from "./axis";

/** Builds a board host holding a `.board` grid of `rows`×`cols` cells. */
function makeBoard(rows: number, cols: number): HTMLElement {
  const host = document.createElement("div");
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
  host.appendChild(board);
  document.body.appendChild(host);
  return host;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("createBoardAxis", () => {
  it("wraps the board host in a .board-axis-zone", () => {
    const host = makeBoard(2, 2);
    createBoardAxis(host);
    expect(host.parentElement!.className).toContain("board-axis-zone");
  });

  it("keeps the label layer outside the board host (screenshot-safe)", () => {
    const host = makeBoard(2, 2);
    createBoardAxis(host);
    const zone = host.parentElement!;
    expect(zone.querySelector(".axis-label-layer")).toBeTruthy();
    expect(host.querySelector(".axis-label-layer")).toBeNull();
  });

  it("starts hidden by default", () => {
    const host = makeBoard(2, 2);
    createBoardAxis(host);
    const layer = host.parentElement!.querySelector(".axis-label-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("starts visible when opts.visible is true", () => {
    const host = makeBoard(2, 2);
    createBoardAxis(host, { visible: true });
    const layer = host.parentElement!.querySelector(".axis-label-layer")!;
    expect(layer.classList.contains("hidden")).toBe(false);
  });

  it("setVisible toggles the layer", () => {
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    const layer = host.parentElement!.querySelector(".axis-label-layer")!;
    axis.setVisible(true);
    expect(layer.classList.contains("hidden")).toBe(false);
    axis.setVisible(false);
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("setRowsCols is callable (reserved for #118)", () => {
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    expect(() => axis.setRowsCols(3, 4)).not.toThrow();
  });

  it("destroy removes the axis wrapper", () => {
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    const zone = host.parentElement!;
    axis.destroy();
    expect(zone.isConnected).toBe(false);
  });
});
