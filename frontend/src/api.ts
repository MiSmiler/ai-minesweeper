// Wire types mirroring the server DTOs (src/server.rs).

import { log } from "./log";

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

/** The full wire snapshot of the game, mirroring the server's StateDto
 * (src/server.rs): the frontend's view of the whole game. Not the Rust-side
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

/** A tool call in a DeepSeek transcript turn. */
export interface TranscriptToolCall {
  name: string;
  arguments: string;
}

/** One model turn: its reasoning, its answer, and any tool calls it made. */
export interface TranscriptTurn {
  reasoning_content: string | null;
  content: string | null;
  tool_calls: TranscriptToolCall[];
}

/** The full transcript returned by `POST /ai/analyze`, mirroring the backend's
 * `SessionResult` (src/ai/session.rs). */
export interface SessionResult {
  steps: TranscriptTurn[];
}

/** Fetches the current game state. */
export async function fetchState(): Promise<GameSnapshot> {
  const res = await fetch("/state");
  if (!res.ok) {
    log.error(`GET /state failed: ${res.status}`);
    throw new Error(`GET /state failed: ${res.status}`);
  }
  return (await res.json()) as GameSnapshot;
}

/** Runs a DeepSeek analysis of the current board and returns the transcript. */
export async function analyzeBoard(): Promise<SessionResult> {
  const res = await fetch("/ai/analyze", { method: "POST" });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    log.error(`POST /ai/analyze failed: ${res.status}`, body);
    throw new Error(
      `POST /ai/analyze failed: ${res.status}${body ? ` — ${body}` : ""}`,
    );
  }
  return (await res.json()) as SessionResult;
}

/** Sends an action and returns the new state. */
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
