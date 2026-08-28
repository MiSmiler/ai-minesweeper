import type { GameSnapshot, Position } from "../api";

/** Row-major offsets of the 8 neighboring Cells. */
const NEIGHBOR_OFFSETS: ReadonlyArray<readonly [number, number]> = [
  [-1, -1],
  [-1, 0],
  [-1, 1],
  [0, -1],
  [0, 1],
  [1, -1],
  [1, 0],
  [1, 1],
];

export function isRevealedNumericCell(
  state: GameSnapshot,
  row: number,
  col: number,
): boolean {
  const cell = state.cells[row * state.cols + col];
  return (
    cell?.state === "revealed" &&
    typeof cell.content === "number" &&
    cell.content > 0
  );
}

/** The Cells a Chord Preview highlights: every Hidden neighbor of a Revealed
 * numeric Cell. Highlights all Hidden neighbors regardless of the Flag count
 * around the Cell (a mismatched count means the Chord would be a no-op, but
 * the preview still shows its scope). Empty unless the Cell is a Revealed
 * numeric Cell. */
export function chordPreviewCells(
  state: GameSnapshot,
  row: number,
  col: number,
): Position[] {
  if (!isRevealedNumericCell(state, row, col)) {
    return [];
  }
  const out: Position[] = [];
  for (const [dr, dc] of NEIGHBOR_OFFSETS) {
    const r = row + dr;
    const c = col + dc;
    if (r < 0 || r >= state.rows || c < 0 || c >= state.cols) {
      continue;
    }
    if (state.cells[r * state.cols + c].state === "hidden") {
      out.push({ row: r, col: c });
    }
  }
  return out;
}
