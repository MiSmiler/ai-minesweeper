import { describe, expect, it } from "vitest";
import type { CellContent, CellState, GameSnapshot, Position } from "./api";
import { chordPreviewCells } from "./chordPreview";

type CellSpec = { state: CellState; content: CellContent };

/** Builds a GameSnapshot from a sparse map of cell specs; default is Hidden. */
function mkState(
  rows: number,
  cols: number,
  cells: Record<string, CellSpec>,
): GameSnapshot {
  const flat: CellSpec[] = [];
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      flat.push(cells[`${r},${c}`] ?? { state: "hidden", content: null });
    }
  }
  return {
    game_state: "playing",
    difficulty: "beginner",
    rows,
    cols,
    flags_remaining: 10,
    elapsed_secs: 0,
    trigger: null,
    cells: flat,
  };
}

/** Orders a set of Cells as "row,col" keys so assertions are order-independent. */
function cellKeys(cells: Position[]): string[] {
  return cells.map((p) => `${p.row},${p.col}`).sort();
}

describe("chordPreviewCells", () => {
  it("highlights every Hidden neighbor of a Revealed numeric Cell", () => {
    const state = mkState(3, 3, { "1,1": { state: "revealed", content: 1 } });
    expect(cellKeys(chordPreviewCells(state, 1, 1))).toEqual([
      "0,0",
      "0,1",
      "0,2",
      "1,0",
      "1,2",
      "2,0",
      "2,1",
      "2,2",
    ]);
  });

  it("is empty on a corner Cell with its three neighbors Hidden", () => {
    const state = mkState(3, 3, { "0,0": { state: "revealed", content: 1 } });
    expect(cellKeys(chordPreviewCells(state, 0, 0))).toEqual([
      "0,1",
      "1,0",
      "1,1",
    ]);
  });

  it("is empty when the Cell is Hidden", () => {
    const state = mkState(2, 2, {});
    expect(cellKeys(chordPreviewCells(state, 0, 0))).toEqual([]);
  });

  it("is empty when the Cell is a Revealed zero Cell", () => {
    const state = mkState(2, 2, { "0,0": { state: "revealed", content: 0 } });
    expect(cellKeys(chordPreviewCells(state, 0, 0))).toEqual([]);
  });

  it("is empty when the Cell is a Revealed Mine", () => {
    const state = mkState(2, 2, {
      "0,0": { state: "revealed", content: "mine" },
    });
    expect(cellKeys(chordPreviewCells(state, 0, 0))).toEqual([]);
  });

  it("is empty when the Cell is Flagged", () => {
    const state = mkState(2, 2, { "0,0": { state: "flagged", content: null } });
    expect(cellKeys(chordPreviewCells(state, 0, 0))).toEqual([]);
  });

  it("excludes Flagged and already Revealed neighbors", () => {
    const state = mkState(3, 3, {
      "1,1": { state: "revealed", content: 3 },
      "0,0": { state: "flagged", content: null },
      "1,0": { state: "revealed", content: 2 },
    });
    expect(cellKeys(chordPreviewCells(state, 1, 1))).toEqual([
      "0,1",
      "0,2",
      "1,2",
      "2,0",
      "2,1",
      "2,2",
    ]);
  });

  it("highlights Hidden neighbors even when the Flag count mismatches (classic behaviour)", () => {
    // A 1 with two Flags: the Chord would be a no-op, but the preview
    // still shows the Hidden neighbors.
    const state = mkState(3, 3, {
      "1,1": { state: "revealed", content: 1 },
      "0,0": { state: "flagged", content: null },
      "0,2": { state: "flagged", content: null },
    });
    expect(cellKeys(chordPreviewCells(state, 1, 1))).toEqual([
      "0,1",
      "1,0",
      "1,2",
      "2,0",
      "2,1",
      "2,2",
    ]);
  });

  it("is empty when every neighbor is Flagged", () => {
    const state = mkState(3, 3, {
      "1,1": { state: "revealed", content: 1 },
      "0,0": { state: "flagged", content: null },
      "0,1": { state: "flagged", content: null },
      "0,2": { state: "flagged", content: null },
      "1,0": { state: "flagged", content: null },
      "1,2": { state: "flagged", content: null },
      "2,0": { state: "flagged", content: null },
      "2,1": { state: "flagged", content: null },
      "2,2": { state: "flagged", content: null },
    });
    expect(cellKeys(chordPreviewCells(state, 1, 1))).toEqual([]);
  });
});
