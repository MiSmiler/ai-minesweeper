// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import type { Position } from "../api";
import { createPreviewRenderer } from "./previewRender";

const pos = (row: number, col: number): Position => ({ row, col });

/** A board with a grid of Cells addressed by data-row/data-col. */
function makeBoard(rows: number, cols: number): HTMLElement {
  const board = document.createElement("div");
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const cell = document.createElement("div");
      cell.className = "cell";
      cell.dataset.row = String(r);
      cell.dataset.col = String(c);
      board.appendChild(cell);
    }
  }
  return board;
}

const isPreviewed = (board: HTMLElement, row: number, col: number): boolean =>
  board
    .querySelector(`[data-row="${row}"][data-col="${col}"]`)!
    .classList.contains("cell-preview");

describe("createPreviewRenderer", () => {
  it("renders the Press Preview Cell", () => {
    const board = makeBoard(3, 3);
    const renderer = createPreviewRenderer(board);
    renderer.render({ kind: "press", pos: pos(2, 2) });
    expect(isPreviewed(board, 2, 2)).toBe(true);
  });

  it("renders the Chord Preview Cells", () => {
    const board = makeBoard(3, 3);
    const renderer = createPreviewRenderer(board);
    renderer.render({
      kind: "chord",
      pos: pos(1, 1),
      cells: [pos(0, 0), pos(0, 1)],
    });
    expect(isPreviewed(board, 0, 0)).toBe(true);
    expect(isPreviewed(board, 0, 1)).toBe(true);
    // The Chord target Cell itself is not part of the highlight.
    expect(isPreviewed(board, 1, 1)).toBe(false);
  });

  it("clears the previous highlight on the next render", () => {
    const board = makeBoard(3, 3);
    const renderer = createPreviewRenderer(board);
    renderer.render({ kind: "press", pos: pos(2, 2) });
    renderer.render({ kind: "chord", pos: pos(1, 1), cells: [pos(0, 0)] });
    renderer.render(null);
    expect(isPreviewed(board, 0, 0)).toBe(false);
    expect(isPreviewed(board, 2, 2)).toBe(false);
  });

  it("keeps the retained Chord Preview through renders until release", () => {
    const board = makeBoard(3, 3);
    const renderer = createPreviewRenderer(board);
    renderer.render({
      kind: "chord",
      pos: pos(1, 1),
      cells: [pos(0, 0), pos(0, 1)],
    });
    renderer.retain();
    // The machine cleared its Preview on release; the layer keeps them
    // until release(), so the Cells do not flash back to Hidden.
    renderer.render(null);
    expect(isPreviewed(board, 0, 0)).toBe(true);
    expect(isPreviewed(board, 0, 1)).toBe(true);
    renderer.release();
    expect(isPreviewed(board, 0, 0)).toBe(false);
    expect(isPreviewed(board, 0, 1)).toBe(false);
  });

  it("keeps the retained Press Preview through renders until release", () => {
    const board = makeBoard(3, 3);
    const renderer = createPreviewRenderer(board);
    renderer.render({ kind: "press", pos: pos(1, 1) });
    renderer.retain();
    renderer.render(null);
    expect(isPreviewed(board, 1, 1)).toBe(true);
    renderer.release();
    expect(isPreviewed(board, 1, 1)).toBe(false);
  });

  it("drops the retained highlight even when the Board was not re-rendered", () => {
    // Mirrors the failed-request path: the Board still holds the old
    // Cells, so release() must remove the retained classes from them.
    const board = makeBoard(3, 3);
    const renderer = createPreviewRenderer(board);
    renderer.render({ kind: "chord", pos: pos(1, 1), cells: [pos(0, 0)] });
    renderer.retain();
    renderer.render(null);
    renderer.release();
    expect(isPreviewed(board, 0, 0)).toBe(false);
  });
});
