// The instantiable game area: builds the HumanPlay-shaped board (difficulty
// bar + status bar + board) and its own independent game client, wiring the
// mouse input the same way the former `main.ts` did. Used by the single page
// layout (ADR-0023) as its one board column (its own client, its own DOM).
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
} from "./api";
import { createGameClient, type GameClient } from "./client";
import {
  cellAtPoint,
  measureBoard,
  type BoardGeometry,
} from "./render/hitTest";
import { log } from "../infra/log";
import type { StatusBarEls } from "./render/snapshotRender";

export interface GameArea {
  /** The root container of the game area (a `.game-body` div). */
  container: HTMLElement;
  /** The stable shell the client renders `.board` into (`.board` itself is
   * re-created on every render). Also the screenshot target and the element
   * the axis wrapper anchors to. */
  boardEl: HTMLElement;
  statusBarEls: StatusBarEls;
  client: GameClient;
  /** Removes the containers, the input listeners and the timer poll. */
  dispose(): void;
}

export interface GameAreaOptions {
  /** Replaces the HTTP action adapter (tests). */
  post?: (action: Action) => Promise<GameSnapshot>;
  /** Replaces the HTTP snapshot adapter (tests). */
  fetchSnapshot?: () => Promise<GameSnapshot>;
  /** Called after a new-game action is confirmed — the layout uses it to end
   * the AI Session. */
  onNewGame?: () => void;
  /** Called before a new game (smiley / difficulty) is confirmed; return false
   * to cancel. The layout uses it to guard a discard of a used Session. */
  beforeNewGame?: () => boolean;
  /** Called after the Board renders a fresh snapshot (initial load and every
   * action response). The layout uses it to keep its axis labels in sync
   * with the live Board size. */
  onRender?: (snapshot: GameSnapshot) => void;
}

const DIFFICULTIES = ["beginner", "intermediate", "expert"] as const;

/** Mounts an independent game area into `root`. */
export function createGameArea(
  root: HTMLElement,
  opts: GameAreaOptions = {},
): GameArea {
  const container = document.createElement("div");
  container.className = "game-body";

  // --- Difficulty bar ---
  const difficultyBar = document.createElement("div");
  difficultyBar.className = "difficulty-bar";
  for (const level of DIFFICULTIES) {
    const btn = document.createElement("button");
    btn.className = "difficulty";
    btn.dataset.difficulty = level;
    btn.textContent = level[0]!.toUpperCase() + level.slice(1);
    difficultyBar.appendChild(btn);
  }

  // --- Status bar ---
  const statusBar = document.createElement("div");
  statusBar.className = "status-bar";
  const counter = document.createElement("div");
  counter.className = "flag-counter";
  const smiley = document.createElement("button");
  smiley.className = "smiley";
  smiley.type = "button";
  smiley.setAttribute("aria-label", "New game");
  smiley.textContent = "🙂";
  const timer = document.createElement("div");
  timer.className = "timer";
  statusBar.append(counter, smiley, timer);

  // --- Board ---
  const boardEl = document.createElement("div");
  boardEl.className = "board-view";
  const panel = document.createElement("div");
  panel.className = "game-panel";
  panel.append(statusBar, boardEl);
  container.append(difficultyBar, panel);
  root.replaceChildren(container);

  const statusBarEls: StatusBarEls = { counter, smiley, timer, difficultyBar };

  const post = async (action: Action): Promise<GameSnapshot> => {
    const snap = await (opts.post ?? postAction)(action);
    if (action.type === "new-game") opts.onNewGame?.();
    return snap;
  };
  const fetchSnapshot = opts.fetchSnapshot ?? fetchSnapshotApi;

  const client = createGameClient({
    boardEl,
    statusBarEls,
    post,
    fetchSnapshot,
    onRender: opts.onRender,
  });

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

  return { container, boardEl, statusBarEls, client, dispose };
}
