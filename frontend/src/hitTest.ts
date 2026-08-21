import type { Pos } from "./api";

/** The Board's hit-testing geometry: the Cell pitch (Cell size + hairline
 * gap) and Cell counts per axis, measured from the rendered grid. The Board
 * is interactively contiguous — the hairline between Cells is decorative and
 * does not affect input (ADR-0008), so every position on the Board maps to a
 * Cell. The Board's origin is not part of the geometry: it can shift with
 * layout, so cellAtPoint reads it fresh from the Board's rect on every call. */
export interface BoardGeometry {
  /** The horizontal Cell pitch — Cell size + hairline, measured from the
   * rendered first-row Cells' left edges. */
  pitchX: number;
  /** The hairline width between Columns, derived from the measured pitch
   * and Cell size; used to split the hairline at its midpoint. */
  gapX: number;
  /** The vertical Cell pitch, measured from the rendered first/last-row
   * Cells' top edges. */
  pitchY: number;
  /** The hairline width between Rows, derived like gapX. */
  gapY: number;
  rows: number;
  cols: number;
}

/** Maps an offset from the Board's first Cell edge to a Cell index. The
 * hairline is split at its midpoint between the two adjacent Cells, so no
 * position on the Board is "no Cell" (ADR-0008). Clamped so offsets past the
 * last Cell (measurement skew) still land in bounds. */
export function offsetToIndex(
  offset: number,
  pitch: number,
  gap: number,
  count: number,
): number {
  return Math.min(
    count - 1,
    Math.max(0, Math.floor((offset + gap / 2) / pitch)),
  );
}

/** Measures the Board's hit-testing geometry from the rendered grid: the
 * first Cell's rect gives the Cell size, and the span between the first and
 * last Cell of the first row (first and last row of the first column) gives
 * the true horizontal (vertical) pitch — Cell size + hairline. The pitch is
 * measured from rendered rects, never from CSS constants or computed styles:
 * the rendered layout is the source of truth (the computed `columnGap` of a
 * non-grid container is `normal`, and subpixel rendering can skew ideal
 * values). Returns null when the Board has no Cells (nothing rendered yet). */
export function measureBoard(board: HTMLElement): BoardGeometry | null {
  const cells = board.querySelectorAll<HTMLElement>(".cell");
  if (cells.length === 0) return null;
  let rows = 0;
  let cols = 0;
  for (const el of cells) {
    rows = Math.max(rows, Number(el.dataset.row) + 1);
    cols = Math.max(cols, Number(el.dataset.col) + 1);
  }
  if (
    !Number.isInteger(rows) ||
    !Number.isInteger(cols) ||
    rows < 1 ||
    cols < 1
  ) {
    return null;
  }
  const first = cells[0].getBoundingClientRect();
  const cellSize = first.width;
  if (!Number.isFinite(cellSize) || cellSize <= 0) return null;
  // The last Cell of the first row and the last row's first Cell (DOM order
  // is row-major, so cells[cols - 1] and cells[(rows - 1) * cols]).
  const lastCol = cells[cols - 1].getBoundingClientRect();
  const lastRow = cells[(rows - 1) * cols].getBoundingClientRect();
  const pitchX = cols > 1 ? (lastCol.left - first.left) / (cols - 1) : cellSize;
  const pitchY = rows > 1 ? (lastRow.top - first.top) / (rows - 1) : cellSize;
  return {
    pitchX,
    gapX: pitchX - cellSize,
    pitchY,
    gapY: pitchY - cellSize,
    rows,
    cols,
  };
}

/** The Cell under the pointer position (client coordinates), mapping through
 * the Board's cached geometry. The Board's origin is measured fresh per call
 * — the board can move (window resize, difficulty switch re-centering) while
 * the pitch and Cell counts stay valid. */
export function cellAtPoint(
  board: HTMLElement,
  x: number,
  y: number,
  geometry: BoardGeometry,
): Pos {
  const rect = board.getBoundingClientRect();
  return {
    row: offsetToIndex(
      y - rect.top,
      geometry.pitchY,
      geometry.gapY,
      geometry.rows,
    ),
    col: offsetToIndex(
      x - rect.left,
      geometry.pitchX,
      geometry.gapX,
      geometry.cols,
    ),
  };
}
