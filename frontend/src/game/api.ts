// Wire types mirroring the server DTOs (src/server/wire.rs).

import { log } from "../infra/log";

export type GameStateName = "ready" | "playing" | "won" | "lost";

/** Whether the game has ended (Won or Lost). An ended game ignores further
 * Board input (issue #50). */
export const isGameEnded = (state: GameStateName): boolean =>
  state === "won" || state === "lost";
export type Difficulty = "beginner" | "intermediate" | "expert";
export type CellState = "hidden" | "flagged" | "revealed";
/** `"mine"` or a neighbor count; null unless the Cell is Revealed. */
export type CellContent = "mine" | number | null;

/** The coordinates of a Cell on the Board, in (row, col) order — the
 * glossary's Position. */
export interface Position {
  row: number;
  col: number;
}

export interface CellView {
  state: CellState;
  content: CellContent;
}

/** The full wire snapshot of the game, mirroring the server's GameSnapshot
 * (src/server/wire.rs): the frontend's view of the whole game. Not the Rust-side
 * GameState phase enum — that phase is the `game_state` field below. */
export interface GameSnapshot {
  game_state: GameStateName;
  difficulty: Difficulty;
  rows: number;
  cols: number;
  flags_remaining: number;
  elapsed_secs: number;
  trigger: Position | null;
  cells: CellView[];
}

export type Action =
  | { type: "reveal"; row: number; col: number }
  | { type: "flag"; row: number; col: number }
  | { type: "chord"; row: number; col: number }
  | { type: "new-game"; difficulty?: Difficulty };

/** Fetches the current snapshot. */
export async function fetchSnapshot(): Promise<GameSnapshot> {
  const res = await fetch("/state");
  if (!res.ok) {
    log.error(`GET /state failed: ${res.status}`);
    throw new Error(`GET /state failed: ${res.status}`);
  }
  return (await res.json()) as GameSnapshot;
}

/** Sends an action and returns the new snapshot. */
export async function postAction(action: Action): Promise<GameSnapshot> {
  const res = await fetch("/action", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(action),
  });
  if (!res.ok) {
    log.error(`POST /action failed: ${res.status}`, action);
    throw new Error(`POST /action failed: ${res.status}`);
  }
  return (await res.json()) as GameSnapshot;
}
