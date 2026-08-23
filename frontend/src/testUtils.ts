import { vi } from "vitest";
import type { CellView, GameSnapshot } from "./api";

/** A laid-out Board's hit-testing fixture values: Cell size CELL, hairline
 * GAP, and the pitch (Cell + hairline) PITCH. Shared by the hitTest and
 * gamePage suites so each describes the same grid geometry. */
export const CELL = 24;
export const GAP = 1.5;
export const PITCH = CELL + GAP;

/** Mocks a Board container's origin — its getBoundingClientRect, width and
 * height ignored — to `(left, top)`. cellAtPoint reads the Board's origin
 * fresh per call, so pointer hit-testing needs only this to work in jsdom. */
export function mockBoardOrigin(
  board: HTMLElement,
  left: number,
  top: number,
): void {
  vi.spyOn(board, "getBoundingClientRect").mockReturnValue(
    new DOMRect(left, top, 0, 0),
  );
}

/** Gives a Board (which already contains `.cell` elements carrying
 * data-row and data-col) a laid-out hit-testing geometry: Cell (r, c) spans [c * PITCH + ox, c * PITCH + ox + CELL) × [r * PITCH + oy, ...)
 * with the Board's origin at `origin` (default the top-left). Mirrors the real
 * layout so pointer events map to Cells. */
export function mockBoardGeometry(
  board: HTMLElement,
  origin: { x: number; y: number } = { x: 0, y: 0 },
): void {
  mockBoardOrigin(board, origin.x, origin.y);
  for (const el of board.querySelectorAll<HTMLElement>(".cell")) {
    const row = Number(el.dataset.row);
    const col = Number(el.dataset.col);
    vi.spyOn(el, "getBoundingClientRect").mockReturnValue(
      new DOMRect(col * PITCH + origin.x, row * PITCH + origin.y, CELL, CELL),
    );
  }
}

/** Builds a minimal 2×2 playing GameSnapshot for tests; override any field. */
export function gameState(over: Partial<GameSnapshot> = {}): GameSnapshot {
  const cells: CellView[] = [];
  for (let i = 0; i < 4; i++) {
    cells.push({ state: "hidden", content: null });
  }
  return {
    game_state: "playing",
    difficulty: "beginner",
    rows: 2,
    cols: 2,
    flags_remaining: 1,
    elapsed_secs: 0,
    trigger: null,
    cells,
    ...over,
  };
}
