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

  it("setRowsCols renders 0-based row labels along the left edge", () => {
    const host = makeBoard(3, 4);
    const axis = createBoardAxis(host);
    axis.setRowsCols(3, 4);
    const rows = host.parentElement!.querySelectorAll<HTMLElement>(".axis-row");
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

  it("setRowsCols renders 0-based col labels along the top edge", () => {
    const host = makeBoard(3, 4);
    const axis = createBoardAxis(host);
    axis.setRowsCols(3, 4);
    const cols = host.parentElement!.querySelectorAll<HTMLElement>(".axis-col");
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
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    axis.setRowsCols(2, 2);
    const zone = host.parentElement!;
    expect(zone.querySelectorAll(".axis-label-layer .axis-row")).toHaveLength(
      2,
    );
    expect(zone.querySelectorAll(".axis-label-layer .axis-col")).toHaveLength(
      2,
    );
    // Never inside the board host (screenshot-safe).
    expect(host.querySelector(".axis-row")).toBeNull();
    expect(host.querySelector(".axis-col")).toBeNull();
  });

  it("setRowsCols re-renders on resize without stale labels", () => {
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    axis.setRowsCols(2, 2);
    expect(host.parentElement!.querySelectorAll(".axis-row")).toHaveLength(2);
    expect(host.parentElement!.querySelectorAll(".axis-col")).toHaveLength(2);

    axis.setRowsCols(3, 5);
    expect(host.parentElement!.querySelectorAll(".axis-row")).toHaveLength(3);
    expect(host.parentElement!.querySelectorAll(".axis-col")).toHaveLength(5);
    expect(
      host.parentElement!.querySelectorAll(".axis-col")[4]!.textContent,
    ).toBe("4");
  });

  it("label visibility follows setVisible", () => {
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    axis.setRowsCols(2, 2);
    const layer =
      host.parentElement!.querySelector<HTMLElement>(".axis-label-layer")!;
    axis.setVisible(true);
    expect(layer.classList.contains("hidden")).toBe(false);
    axis.setVisible(false);
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("destroy removes the axis wrapper", () => {
    const host = makeBoard(2, 2);
    const axis = createBoardAxis(host);
    const zone = host.parentElement!;
    axis.destroy();
    expect(zone.isConnected).toBe(false);
  });
});
