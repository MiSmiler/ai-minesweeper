// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cellAtPoint, measureBoard, offsetToIndex } from "./hitTest";
import {
  CELL,
  GAP,
  PITCH,
  mockBoardGeometry,
  mockBoardOrigin,
} from "./testUtils";

afterEach(() => {
  vi.restoreAllMocks();
});

/** Builds a Board whose Cells report the real layout: Cell size CELL, hairline
 * GAP, so Cell (r, c) spans [c * PITCH, c * PITCH + CELL) × [r * PITCH, ...). */
function laidOutBoard(rows: number, cols: number): HTMLElement {
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
  mockBoardGeometry(board);
  return board;
}

describe("offsetToIndex", () => {
  it("maps positions inside a Cell to that Cell", () => {
    expect(offsetToIndex(0, PITCH, GAP, 9)).toBe(0);
    expect(offsetToIndex(CELL - 0.1, PITCH, GAP, 9)).toBe(0);
    expect(offsetToIndex(PITCH, PITCH, GAP, 9)).toBe(1);
    expect(offsetToIndex(PITCH + CELL - 0.1, PITCH, GAP, 9)).toBe(1);
  });

  it("splits the hairline at its midpoint between the two adjacent Cells", () => {
    // Cell 0 spans [0, 24), the hairline [24, 25.5) with midpoint 24.75.
    expect(offsetToIndex(CELL, PITCH, GAP, 9)).toBe(0);
    expect(offsetToIndex(CELL + GAP / 2 - 0.1, PITCH, GAP, 9)).toBe(0);
    expect(offsetToIndex(CELL + GAP / 2 + 0.1, PITCH, GAP, 9)).toBe(1);
  });

  it("clamps out-of-range offsets to the Board edges", () => {
    expect(offsetToIndex(-5, PITCH, GAP, 9)).toBe(0);
    expect(offsetToIndex(1e9, PITCH, GAP, 9)).toBe(8);
  });

  it("works with a zero gap (contiguous Cells, no hairline)", () => {
    expect(offsetToIndex(CELL - 0.1, CELL, 0, 9)).toBe(0);
    expect(offsetToIndex(CELL, CELL, 0, 9)).toBe(1);
  });
});

describe("measureBoard", () => {
  it("derives the pitch from the rendered Cell rects, not CSS constants", () => {
    const board = laidOutBoard(2, 3);
    expect(measureBoard(board)).toEqual({
      pitchX: PITCH,
      gapX: GAP,
      pitchY: PITCH,
      gapY: GAP,
      rows: 2,
      cols: 3,
    });
  });

  it("keeps a 16-column Board's pitch exact so no cumulative drift appears", () => {
    // Regression: a pitch derived from CSS constants (or a computed gap of 0)
    // drifts one Cell right from column 8 onward; the rect-measured pitch
    // must stay exact across the whole Intermediate Board.
    const board = laidOutBoard(16, 16);
    const geometry = measureBoard(board)!;
    expect(geometry.pitchX).toBeCloseTo(PITCH, 10);
    expect(geometry.gapX).toBeCloseTo(GAP, 10);
  });

  it("treats a single-row/column Board as having no hairline", () => {
    const board = laidOutBoard(1, 1);
    expect(measureBoard(board)).toEqual({
      pitchX: CELL,
      gapX: 0,
      pitchY: CELL,
      gapY: 0,
      rows: 1,
      cols: 1,
    });
  });

  it("returns null when the Board has no Cells", () => {
    expect(measureBoard(document.createElement("div"))).toBeNull();
  });
});

describe("cellAtPoint", () => {
  const geometry = {
    pitchX: PITCH,
    gapX: GAP,
    pitchY: PITCH,
    gapY: GAP,
    rows: 2,
    cols: 3,
  };
  const boardAt = (left: number, top: number): HTMLElement => {
    const board = document.createElement("div");
    mockBoardOrigin(board, left, top);
    return board;
  };

  it("maps a pointer position to the Cell under it, from the Board's origin", () => {
    const board = boardAt(10, 20);
    expect(cellAtPoint(board, 10 + PITCH + 5, 20 + 5, geometry)).toEqual({
      row: 0,
      col: 1,
    });
    expect(cellAtPoint(board, 10 + 5, 20 + PITCH + 5, geometry)).toEqual({
      row: 1,
      col: 0,
    });
  });

  it("splits the hairline at its midpoint", () => {
    const board = boardAt(0, 0);
    expect(cellAtPoint(board, CELL + GAP / 2 - 0.1, 0, geometry)).toEqual({
      row: 0,
      col: 0,
    });
    expect(cellAtPoint(board, CELL + GAP / 2 + 0.1, 0, geometry)).toEqual({
      row: 0,
      col: 1,
    });
  });

  it("maps the last column of a wide Board to itself, not the one past it", () => {
    // Regression: clicking the second-to-last Cell of an Intermediate Board
    // must hit that Cell — the rect-measured pitch keeps the mapping exact.
    const board = boardAt(0, 0);
    const wide = { ...geometry, cols: 16 };
    // Cell 14's center: 14 * PITCH + CELL / 2.
    expect(cellAtPoint(board, 14 * PITCH + CELL / 2, 0, wide)).toEqual({
      row: 0,
      col: 14,
    });
    expect(cellAtPoint(board, 15 * PITCH + CELL / 2, 0, wide)).toEqual({
      row: 0,
      col: 15,
    });
  });
});
