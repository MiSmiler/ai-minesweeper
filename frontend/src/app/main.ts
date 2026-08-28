import {
  fetchState,
  postAction,
  type GameSnapshot,
  type Position,
} from "../game/api";
import { createGameClient } from "../game/client";
import {
  cellAtPoint,
  measureBoard,
  type BoardGeometry,
} from "../game/render/hitTest";
import { log } from "../infra/log";
import type { TopBarEls } from "../game/render/render";
import "../style.css";

const boardEl = document.getElementById("board")!;
const topBarEls: TopBarEls = {
  counter: document.getElementById("counter")!,
  smiley: document.getElementById("smiley")!,
  timer: document.getElementById("timer")!,
  difficultyRow: document.querySelector(".difficulty-row")!,
};

/** The client module: owns the cached state, the gesture machine, the action
 * controller, the previews, the Smiley Button, and all rendering. This file
 * is a thin DOM adapter — it translates mouse events into abstract input and
 * delegates the rest. */
const client = createGameClient({
  boardEl,
  topBarEls,
  post: postAction,
  fetchState,
});

/** Tracks the last hit-tested Cell so pointer-move events are only
 * dispatched when the pointer actually crosses onto/off a Cell — pointermove
 * fires far more often than the Chord Preview needs to change. */
let lastPointerCell: Position | null = null;

/** The Board's cached hit-testing geometry (Cell pitch, hairline gap, Cell
 * counts). Re-measured when the rendered Cell count changes (a difficulty
 * switch re-renders the Board) or before the first event; the Board's origin
 * is read fresh per event since layout can shift. */
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
 * geometry (defensive — the listeners are registered only after the initial
 * state load, so this should not happen in practice). */
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

function onBoardMouseDown(ev: MouseEvent): void {
  if (ev.button === 2) {
    handleRightDown(ev);
  } else if (ev.button === 0) {
    handleLeftDown(ev);
  }
}

function handleLeftDown(ev: MouseEvent): void {
  const pos = posAt(ev);
  if (!pos) return;
  ev.preventDefault();
  client.handleInput({ kind: "left-down", pos });
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

async function main(): Promise<void> {
  try {
    await client.init();
    boardEl.addEventListener("mousedown", onBoardMouseDown);
    boardEl.addEventListener("pointermove", onBoardPointerMove);
    boardEl.addEventListener("pointerleave", onBoardPointerLeave);
    boardEl.addEventListener("contextmenu", onContextMenu);
    document.getElementById("app")!.addEventListener("click", onAppClick);
    window.addEventListener("mouseup", onWindowMouseUp);
    window.addEventListener("blur", onWindowBlur);
    window.setInterval(() => void client.pollTimer(), 1000);
  } catch (err) {
    const message = err instanceof Error ? err.message : err;
    log.error(`Failed to load game: ${message}`);
    boardEl.textContent = `Failed to load game: ${message}`;
  }
}

main();
