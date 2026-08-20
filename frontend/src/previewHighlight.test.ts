// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import type { Pos } from "./api";
import type { ChordPreview } from "./gesture";
import { createPreviewLayer } from "./previewHighlight";

const pos = (row: number, col: number): Pos => ({ row, col });

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

describe("createPreviewLayer", () => {
  it("renders the Press Preview Cell and the Chord Preview Cells", () => {
    const board = makeBoard(3, 3);
    const layer = createPreviewLayer(board);
    const chord: ChordPreview = {
      pos: pos(1, 1),
      cells: [pos(0, 0), pos(0, 1)],
    };
    layer.render(chord, pos(2, 2));
    expect(isPreviewed(board, 2, 2)).toBe(true);
    expect(isPreviewed(board, 0, 0)).toBe(true);
    expect(isPreviewed(board, 0, 1)).toBe(true);
    // The Chord target Cell itself is not part of the highlight.
    expect(isPreviewed(board, 1, 1)).toBe(false);
  });

  it("clears the previous highlight on the next render", () => {
    const board = makeBoard(3, 3);
    const layer = createPreviewLayer(board);
    layer.render({ pos: pos(1, 1), cells: [pos(0, 0)] }, pos(2, 2));
    layer.render(null, null);
    expect(isPreviewed(board, 0, 0)).toBe(false);
    expect(isPreviewed(board, 2, 2)).toBe(false);
  });

  it("keeps the retained Chord Preview through renders until release", () => {
    const board = makeBoard(3, 3);
    const layer = createPreviewLayer(board);
    layer.render({ pos: pos(1, 1), cells: [pos(0, 0), pos(0, 1)] }, null);
    layer.retain();
    // The machine cleared its Previews on release; the layer keeps them
    // until release(), so the Cells do not flash back to Hidden.
    layer.render(null, null);
    expect(isPreviewed(board, 0, 0)).toBe(true);
    expect(isPreviewed(board, 0, 1)).toBe(true);
    layer.release();
    expect(isPreviewed(board, 0, 0)).toBe(false);
    expect(isPreviewed(board, 0, 1)).toBe(false);
  });

  it("keeps the retained Press Preview through renders until release", () => {
    const board = makeBoard(3, 3);
    const layer = createPreviewLayer(board);
    layer.render(null, pos(1, 1));
    layer.retain();
    layer.render(null, null);
    expect(isPreviewed(board, 1, 1)).toBe(true);
    layer.release();
    expect(isPreviewed(board, 1, 1)).toBe(false);
  });

  it("drops the retained highlight even when the Board was not re-rendered", () => {
    // Mirrors the failed-request path: the Board still holds the old
    // Cells, so release() must remove the retained classes from them.
    const board = makeBoard(3, 3);
    const layer = createPreviewLayer(board);
    layer.render({ pos: pos(1, 1), cells: [pos(0, 0)] }, null);
    layer.retain();
    layer.render(null, null);
    layer.release();
    expect(isPreviewed(board, 0, 0)).toBe(false);
  });
});
