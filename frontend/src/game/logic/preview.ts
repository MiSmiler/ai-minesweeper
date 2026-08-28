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

/** A transient highlight shown during a Gesture. At most one Preview is active
 * at a time. `press` — the Press Preview (the Cell under the pointer); `chord` —
 * the Chord Preview (a Revealed numeric Cell's unflagged neighbors). Pure data. */
export type Preview =
  | { kind: "press"; pos: Position }
  | { kind: "chord"; pos: Position; cells: Position[] };

/** The Press Preview over the pointer Cell — always `kind: "press"`. Trivial: the
 * pressed Cell's position is given, never derived from game state. */
export function pressPreview(pos: Position): Preview {
  return { kind: "press", pos };
}

/** The Chord Preview over the Cell at (row, col), or null when it has no scope
 * (not a Revealed numeric Cell, or every neighbor is Flagged/Revealed). */
export function chordPreview(
  state: GameSnapshot,
  pos: Position,
): Preview | null {
  if (!isRevealedNumericCell(state, pos.row, pos.col)) return null;
  const cells = chordPreviewCells(state, pos.row, pos.col);
  return cells.length ? { kind: "chord", pos, cells } : null;
}

function isRevealedNumericCell(
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
function chordPreviewCells(
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
