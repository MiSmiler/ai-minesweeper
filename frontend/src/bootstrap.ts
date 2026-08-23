import {
  fetchState,
  postAction,
  type GameSnapshot,
  type Position,
} from "./api";
import { createGameClient, type GameClient } from "./client";
import { cellAtPoint, measureBoard, type BoardGeometry } from "./hitTest";
import type { TopBarEls } from "./render";

/** The Board + top-bar elements a game page renders into. */
export interface GamePageEls {
  boardEl: HTMLElement;
  topBarEls: TopBarEls;
}

/** Live handles a booted game page hands back for post-boot wiring — e.g. an
 * AI panel built on top of the Board. */
export interface GamePage {
  client: GameClient;
  boardEl: HTMLElement;
  topBarEls: TopBarEls;
}

/** Resolves the Board + top-bar elements a game page needs, from its mount
 * container — shared by the human and ai pages so each can host the same
 * client. Throws when the markup is missing a required element. */
export function gamePageEls(container: HTMLElement): GamePageEls {
  const boardEl = container.querySelector<HTMLElement>("#board");
  const counter = container.querySelector<HTMLElement>("#counter");
  const smiley = container.querySelector<HTMLElement>("#smiley");
  const timer = container.querySelector<HTMLElement>("#timer");
  const difficultyRow = container.querySelector<HTMLElement>(".difficulty-row");
  if (!boardEl || !counter || !smiley || !timer || !difficultyRow) {
    throw new Error("game page markup is missing required elements");
  }
  return { boardEl, topBarEls: { counter, smiley, timer, difficultyRow } };
}

/** Boots a game page inside `container`: creates the shared client, wires the
 * Board input + top-bar controls, loads the initial state, and starts the
 * timer. Both the human and ai pages use this — the ai page passes a container
 * that also hosts its AI panel. On a load failure it renders the message on
 * the Board and rethrows. */
export async function bootstrapGamePage(
  container: HTMLElement,
): Promise<GamePage> {
  const { boardEl, topBarEls } = gamePageEls(container);
  const client = createGameClient({
    boardEl,
    topBarEls,
    post: postAction,
    fetchState,
  });

  /** Tracks the last hit-tested Cell so pointer-move events are only
   * dispatched when the pointer actually crosses onto/off a Cell. */
  let lastPointerCell: Position | null = null;

  /** The Board's cached hit-testing geometry (Cell pitch, hairline gap, Cell
   * counts). Re-measured when the rendered Cell count changes (a difficulty
   * switch re-renders the Board) or before the first event. */
  let boardGeometryCache: BoardGeometry | null = null;
  let boardCellCount = 0;

  function boardGeometry(): BoardGeometry | null {
    const count = boardEl.querySelectorAll(".cell").length;
    if (count !== boardCellCount || boardGeometryCache === null) {
      boardGeometryCache = measureBoard(boardEl);
      boardCellCount = count;
    }
    return boardGeometryCache;
  }

  /** The Cell under the pointer, or null when the Board has no measured
   * geometry (defensive — listeners are registered only after the initial
   * state load). */
  function posAt(ev: MouseEvent | PointerEvent): Position | null {
    const geometry = boardGeometry();
    return geometry
      ? cellAtPoint(boardEl, ev.clientX, ev.clientY, geometry)
      : null;
  }

  function handleRightDown(ev: MouseEvent): void {
    const pos = posAt(ev);
    if (pos) {
      ev.preventDefault();
      client.handleInput({ kind: "right-down", pos });
    } else {
      // The press is still remembered for the chord gesture, even off a Cell.
      client.handleInput({ kind: "right-down", pos: null });
    }
  }

  function handleLeftDown(ev: MouseEvent): void {
    const pos = posAt(ev);
    if (!pos) return;
    ev.preventDefault();
    client.handleInput({ kind: "left-down", pos });
  }

  function onBoardMouseDown(ev: MouseEvent): void {
    if (ev.button === 2) {
      handleRightDown(ev);
    } else if (ev.button === 0) {
      handleLeftDown(ev);
    }
  }

  function onWindowMouseUp(ev: MouseEvent): void {
    if (ev.button === 2) {
      client.handleInput({ kind: "right-up" });
    } else if (ev.button === 0) {
      client.handleInput({ kind: "left-up" });
    }
  }

  function onWindowBlur(): void {
    client.handleInput({ kind: "blur" });
  }

  function onBoardPointerLeave(): void {
    // Reset the move guard so re-entering the Board reports the first Cell
    // even if it is the one the pointer left on (restoring a cleared Preview).
    lastPointerCell = null;
    client.handleInput({ kind: "pointer-leave" });
  }

  function onBoardPointerMove(ev: PointerEvent): void {
    const pos = posAt(ev);
    // Equal when both are null (the Board has no measured geometry — defensive)
    // or both name the same Cell — anything else is a change worth dispatching.
    const sameCell =
      (pos === null && lastPointerCell === null) ||
      (pos !== null &&
        lastPointerCell !== null &&
        pos.row === lastPointerCell.row &&
        pos.col === lastPointerCell.col);
    if (sameCell) return;
    lastPointerCell = pos;
    client.handleInput({ kind: "pointer-move", pos });
  }

  function onContextMenu(ev: Event): void {
    ev.preventDefault();
  }

  function onAppClick(ev: Event): void {
    const target = ev.target as HTMLElement;
    const difficultyBtn = target.closest<HTMLElement>("[data-difficulty]");
    if (difficultyBtn) {
      // A difficulty button starts a fresh game of that difficulty.
      client.newGame(
        difficultyBtn.dataset.difficulty as GameSnapshot["difficulty"],
      );
      return;
    }
    if (target.closest("#smiley")) {
      // The Smiley Button restarts with the current difficulty.
      client.newGame();
    }
  }

  try {
    await client.init();
    boardEl.addEventListener("mousedown", onBoardMouseDown);
    boardEl.addEventListener("pointermove", onBoardPointerMove);
    boardEl.addEventListener("pointerleave", onBoardPointerLeave);
    boardEl.addEventListener("contextmenu", onContextMenu);
    container.addEventListener("click", onAppClick);
    window.addEventListener("mouseup", onWindowMouseUp);
    window.addEventListener("blur", onWindowBlur);
    window.setInterval(() => void client.pollTimer(), 1000);
    return { client, boardEl, topBarEls };
  } catch (err) {
    const message = err instanceof Error ? err.message : err;
    boardEl.textContent = `Failed to load game: ${message}`;
    throw err;
  }
}
