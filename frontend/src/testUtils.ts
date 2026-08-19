import type { CellView, GameState } from "./api";

/** Builds a minimal 2×2 playing GameState for tests; override any field. */
export function gameState(over: Partial<GameState> = {}): GameState {
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
