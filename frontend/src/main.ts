import {
  fetchState,
  postAction,
  type Action,
  type GameState,
  type Pos,
} from "./api";
import { chordPreviewCells, isRevealedNumericCell } from "./chordPreview";
import { createActionController } from "./controller";
import {
  createGestureMachine,
  type ChordTarget,
  type GestureEvent,
  type GestureOutput,
} from "./gesture";
import { log } from "./log";
import { formatTimer, renderBoard, renderTopBar } from "./render";
import "./style.css";

const boardEl = document.getElementById("board")!;

let state: GameState | null = null;

const gesture = createGestureMachine();
const controller = createActionController(postAction);

/** Sends an Action through the controller and renders the fresh state. Only
 * the latest action's result is ever rendered — stale responses are dropped
 * by the controller (see createActionController). */
async function applyAndRender(action: Action): Promise<void> {
  const next = await controller.apply(action);
  if (next) {
    state = next;
    renderBoard(state, boardEl);
    renderTopBar(state);
  }
}

/** Renders the Chord Preview: highlights exactly the Cells in `preview`. */
function renderPreview(preview: GestureOutput["preview"]): void {
  boardEl
    .querySelectorAll(".cell-chord-preview")
    .forEach((el) => el.classList.remove("cell-chord-preview"));
  if (preview) {
    for (const pos of preview.cells) {
      boardEl
        .querySelector(`[data-row="${pos.row}"][data-col="${pos.col}"]`)
        ?.classList.add("cell-chord-preview");
    }
  }
}

/** Feeds a gesture event to the machine and applies its output. State
 * transitions are traced at `debug` so gesture problems are diagnosable from
 * the console alone (the machine itself stays pure). */
function dispatchGesture(event: GestureEvent): void {
  const out = gesture.handle(event);
  if (out.transition) {
    log.debug(`gesture ${out.transition}`, {
      event: event.kind,
      ...(out.action ? { action: out.action } : {}),
    });
  }
  renderPreview(out.preview);
  if (out.action) {
    void applyAndRender(out.action);
  }
}

/** Polls the state once per second to drive the Timer; the counter and
 * banner come from action responses, which are always fresher. */
async function pollTimer(): Promise<void> {
  try {
    const next = await fetchState();
    const timer = document.getElementById("timer")!;
    timer.textContent = formatTimer(next.elapsed_secs);
  } catch {
    // Transient network errors are ignored; the next poll retries.
  }
}

function cellAt(ev: MouseEvent): HTMLElement | null {
  return (ev.target as HTMLElement).closest<HTMLElement>(".cell");
}

function cellPos(cell: HTMLElement): Pos {
  return { row: Number(cell.dataset.row), col: Number(cell.dataset.col) };
}

/** Builds the hit-test payload for the gesture machine: the Cell's Preview
 * scope plus whether it is a Revealed numeric Cell (the arming criterion). */
function chordTarget(state: GameState, pos: Pos): ChordTarget {
  return {
    pos,
    previewCells: chordPreviewCells(state, pos.row, pos.col),
    isNumericCell: isRevealedNumericCell(state, pos.row, pos.col),
  };
}

function handleRightDown(ev: MouseEvent): void {
  const cell = cellAt(ev);
  if (cell) {
    ev.preventDefault();
    dispatchGesture({
      kind: "right-down",
      cell: state ? chordTarget(state, cellPos(cell)) : null,
    });
  } else {
    // The press is still remembered for the chord gesture, even off a Cell.
    dispatchGesture({ kind: "right-down", cell: null });
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
  const cell = cellAt(ev);
  if (!cell || !state) return;
  ev.preventDefault();
  dispatchGesture({
    kind: "left-down",
    cell: chordTarget(state, cellPos(cell)),
  });
}

function onWindowMouseUp(ev: MouseEvent): void {
  if (ev.button === 2) {
    dispatchGesture({ kind: "right-up" });
  } else if (ev.button === 0) {
    dispatchGesture({ kind: "left-up" });
  }
}

function onWindowBlur(): void {
  dispatchGesture({ kind: "blur" });
}

/** Tracks the last hit-tested Cell so pointer-move events are only
 * dispatched when the pointer actually crosses onto/off a Cell — pointermove
 * fires far more often than the Chord Preview needs to change. */
let lastPointerCell: Pos | null = null;

function onBoardPointerLeave(): void {
  // Reset the move guard so re-entering the Board reports the first Cell
  // even if it is the one the pointer left on (restoring a cleared Preview).
  lastPointerCell = null;
  dispatchGesture({ kind: "pointer-leave" });
}

function onBoardPointerMove(ev: PointerEvent): void {
  const cell = cellAt(ev);
  const pos = cell ? cellPos(cell) : null;
  // Equal when both are null (moving within Board space that is no Cell) or
  // both name the same Cell — anything else is a change worth dispatching.
  const sameCell =
    (pos === null && lastPointerCell === null) ||
    (pos !== null &&
      lastPointerCell !== null &&
      pos.row === lastPointerCell.row &&
      pos.col === lastPointerCell.col);
  if (sameCell) return;
  lastPointerCell = pos;
  if (!state) return;
  dispatchGesture({
    kind: "pointer-move",
    cell: pos ? chordTarget(state, pos) : null,
  });
}

function onContextMenu(ev: Event): void {
  ev.preventDefault();
}

function onTopBarClick(ev: Event): void {
  const target = ev.target as HTMLElement;
  const difficultyBtn = target.closest<HTMLElement>("[data-difficulty]");
  if (difficultyBtn) {
    // A difficulty button starts a fresh game of that difficulty.
    void applyAndRender({
      type: "new-game",
      difficulty: difficultyBtn.dataset.difficulty as GameState["difficulty"],
    });
    return;
  }
  if (target.closest("#new-game")) {
    // New Game restarts with the current difficulty.
    void applyAndRender({ type: "new-game" });
  }
}

async function main(): Promise<void> {
  try {
    state = await fetchState();
    renderBoard(state, boardEl);
    renderTopBar(state);
    boardEl.addEventListener("mousedown", onBoardMouseDown);
    boardEl.addEventListener("pointermove", onBoardPointerMove);
    boardEl.addEventListener("pointerleave", onBoardPointerLeave);
    boardEl.addEventListener("contextmenu", onContextMenu);
    document
      .querySelector(".top-bar")!
      .addEventListener("click", onTopBarClick);
    window.addEventListener("mouseup", onWindowMouseUp);
    window.addEventListener("blur", onWindowBlur);
    window.setInterval(() => void pollTimer(), 1000);
  } catch (err) {
    const message = err instanceof Error ? err.message : err;
    log.error(`Failed to load game: ${message}`);
    boardEl.textContent = `Failed to load game: ${message}`;
  }
}

main();
