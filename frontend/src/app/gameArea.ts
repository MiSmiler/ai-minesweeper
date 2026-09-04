// The instantiable game area: builds the SinglePlay-shaped board (difficulty
// row + top bar + board) and its own independent game client, wiring the mouse
// input the same way the former `main.ts` did. Used by `composeSingleMode`
// and by the `AiGuide` mode's board zone (a full copy, its own client).
//
// This is the seam where the game slice (`createGameClient`) meets the `app/`
// composition. It owns the DOM it creates, the input listeners it registers,
// the timer poll, and the cleanup; `dispose()` tears all of it down.

import {
  fetchSnapshot as fetchSnapshotApi,
  postAction,
  type Action,
  type GameSnapshot,
  type Position,
} from "../game/api";
import { createGameClient, type GameClient } from "../game/client";
import {
  cellAtPoint,
  measureBoard,
  type BoardGeometry,
} from "../game/render/hitTest";
import { log } from "../infra/log";
import type { TopBarEls } from "../game/render/snapshotRender";

export interface GameArea {
  /** The root container of the game area (a `.game-area` div). */
  container: HTMLElement;
  /** The Board container the client renders into. */
  boardEl: HTMLElement;
  topBarEls: TopBarEls;
  client: GameClient;
  /** Removes the containers, the input listeners and the timer poll. */
  dispose(): void;
}

export interface GameAreaOptions {
  /** Replaces the HTTP action adapter (tests). */
  post?: (action: Action) => Promise<GameSnapshot>;
  /** Replaces the HTTP snapshot adapter (tests). */
  fetchSnapshot?: () => Promise<GameSnapshot>;
  /** Called after a new-game action is confirmed — the mode uses it to reset
   * per-game state (history, session id). */
  onNewGame?: () => void;
  /** Called before a new game (smiley / difficulty) is confirmed; return false
   * to cancel. The AiGuide mode uses it to guard a guide-history discard. */
  beforeNewGame?: () => boolean;
}

const DIFFICULTIES = ["beginner", "intermediate", "expert"] as const;

/** Mounts an independent game area into `root`. */
export function createGameArea(
  root: HTMLElement,
  opts: GameAreaOptions = {},
): GameArea {
  const container = document.createElement("div");
  container.className = "game-area";

  // --- Difficulty row ---
  const difficultyRow = document.createElement("div");
  difficultyRow.className = "difficulty-row";
  for (const level of DIFFICULTIES) {
    const btn = document.createElement("button");
    btn.className = "difficulty";
    btn.dataset.difficulty = level;
    btn.textContent = level[0]!.toUpperCase() + level.slice(1);
    difficultyRow.appendChild(btn);
  }

  // --- Top bar ---
  const topBar = document.createElement("div");
  topBar.className = "game-top-bar";
  const counter = document.createElement("div");
  counter.className = "led";
  counter.dataset.role = "counter";
  const smiley = document.createElement("button");
  smiley.className = "smiley";
  smiley.type = "button";
  smiley.setAttribute("aria-label", "New game");
  smiley.textContent = "🙂";
  const timer = document.createElement("div");
  timer.className = "led";
  timer.dataset.role = "timer";
  topBar.append(counter, smiley, timer);

  // --- Board ---
  const boardEl = document.createElement("div");
  boardEl.className = "board-host";
  const frame = document.createElement("div");
  frame.className = "game-frame";
  frame.append(topBar, boardEl);
  container.append(difficultyRow, frame);
  root.replaceChildren(container);

  const topBarEls: TopBarEls = { counter, smiley, timer, difficultyRow };

  const post = async (action: Action): Promise<GameSnapshot> => {
    const snap = await (opts.post ?? postAction)(action);
    if (action.type === "new-game") opts.onNewGame?.();
    return snap;
  };
  const fetchSnapshot = opts.fetchSnapshot ?? fetchSnapshotApi;

  const client = createGameClient({ boardEl, topBarEls, post, fetchSnapshot });

  /** Tracks the last hit-tested Cell so pointer-move events are only dispatched
   * when the pointer actually crosses onto/off a Cell. */
  let lastPointerCell: Position | null = null;
  let boardGeometryCache: BoardGeometry | null = null;
  let boardCellCount = 0;

  const boardGeometry = (): BoardGeometry | null => {
    const count = boardEl.querySelectorAll(".cell").length;
    if (count !== boardCellCount || boardGeometryCache === null) {
      boardGeometryCache = measureBoard(boardEl);
      boardCellCount = count;
    }
    return boardGeometryCache;
  };

  const posAt = (ev: MouseEvent | PointerEvent): Position | null => {
    const geometry = boardGeometry();
    return geometry
      ? cellAtPoint(boardEl, ev.clientX, ev.clientY, geometry)
      : null;
  };

  const handleRightDown = (ev: MouseEvent): void => {
    const pos = posAt(ev);
    ev.preventDefault();
    if (pos) {
      client.handleInput({ kind: "right-down", pos });
    } else {
      client.handleInput({ kind: "right-down", pos: null });
    }
  };

  const handleLeftDown = (ev: MouseEvent): void => {
    const pos = posAt(ev);
    if (!pos) return;
    ev.preventDefault();
    client.handleInput({ kind: "left-down", pos });
  };

  const onBoardMouseDown = (ev: MouseEvent): void => {
    if (ev.button === 2) {
      handleRightDown(ev);
    } else if (ev.button === 0) {
      handleLeftDown(ev);
    }
  };

  const onWindowMouseUp = (ev: MouseEvent): void => {
    if (ev.button === 2) {
      client.handleInput({ kind: "right-up" });
    } else if (ev.button === 0) {
      client.handleInput({ kind: "left-up" });
    }
  };

  const onWindowBlur = (): void => {
    client.handleInput({ kind: "blur" });
  };

  const onBoardPointerLeave = (): void => {
    lastPointerCell = null;
    client.handleInput({ kind: "pointer-leave" });
  };

  const onBoardPointerMove = (ev: PointerEvent): void => {
    const pos = posAt(ev);
    const sameCell =
      (pos === null && lastPointerCell === null) ||
      (pos !== null &&
        lastPointerCell !== null &&
        pos.row === lastPointerCell.row &&
        pos.col === lastPointerCell.col);
    if (sameCell) return;
    lastPointerCell = pos;
    client.handleInput({ kind: "pointer-move", pos });
  };

  const onContextMenu = (ev: Event): void => {
    ev.preventDefault();
  };

  const onAppClick = (ev: MouseEvent): void => {
    const target = ev.target as HTMLElement;
    const difficultyBtn = target.closest<HTMLElement>("[data-difficulty]");
    if (difficultyBtn) {
      if (opts.beforeNewGame && !opts.beforeNewGame()) return;
      client.newGame(
        difficultyBtn.dataset.difficulty as GameSnapshot["difficulty"],
      );
      return;
    }
    if (target.closest(".smiley")) {
      if (opts.beforeNewGame && !opts.beforeNewGame()) return;
      client.newGame();
    }
  };

  // Registered directly on each concrete element so TypeScript picks the
  // DOM overload whose event type matches the literal string (a helper taking
  // a generic `EventListener` would reject the more specific handlers under
  // `strictFunctionTypes`).
  const cleanup: Array<() => void> = [];
  let timerId: number | null = null;

  const start = async (): Promise<void> => {
    try {
      await client.init();
      boardEl.addEventListener("mousedown", onBoardMouseDown);
      cleanup.push(() =>
        boardEl.removeEventListener("mousedown", onBoardMouseDown),
      );
      boardEl.addEventListener("pointermove", onBoardPointerMove);
      cleanup.push(() =>
        boardEl.removeEventListener("pointermove", onBoardPointerMove),
      );
      boardEl.addEventListener("pointerleave", onBoardPointerLeave);
      cleanup.push(() =>
        boardEl.removeEventListener("pointerleave", onBoardPointerLeave),
      );
      boardEl.addEventListener("contextmenu", onContextMenu);
      cleanup.push(() =>
        boardEl.removeEventListener("contextmenu", onContextMenu),
      );
      container.addEventListener("click", onAppClick);
      cleanup.push(() => container.removeEventListener("click", onAppClick));
      window.addEventListener("mouseup", onWindowMouseUp);
      cleanup.push(() =>
        window.removeEventListener("mouseup", onWindowMouseUp),
      );
      window.addEventListener("blur", onWindowBlur);
      cleanup.push(() => window.removeEventListener("blur", onWindowBlur));
      timerId = window.setInterval(() => void client.pollTimer(), 1000);
      cleanup.push(() => {
        if (timerId !== null) window.clearInterval(timerId);
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : err;
      log.error(`Failed to load game: ${message}`);
      boardEl.textContent = `Failed to load game: ${message}`;
    }
  };
  void start();

  const dispose = (): void => {
    for (const fn of cleanup) fn();
    container.remove();
  };

  return { container, boardEl, topBarEls, client, dispose };
}
