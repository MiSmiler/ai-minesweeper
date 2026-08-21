import {
  fetchState,
  isGameEnded,
  postAction,
  type Action,
  type GameState,
  type Pos,
} from "./api";
import { chordPreviewCells, isRevealedNumericCell } from "./chordPreview";
import { createActionController } from "./controller";
import {
  createGestureMachine,
  type CellHit,
  type GestureEvent,
} from "./gesture";
import { log } from "./log";
import { createPreviewLayer } from "./previewHighlight";
import {
  formatTimer,
  renderBoard,
  renderTopBar,
  SmileyFace,
  smileyFace,
} from "./render";
import "./style.css";

const boardEl = document.getElementById("board")!;

let state: GameState | null = null;

const gesture = createGestureMachine();
const controller = createActionController(postAction);
const previewLayer = createPreviewLayer(boardEl);

/** Whether a press is currently held over the Board, as reported by the last
 * gesture dispatch — kept so an action response re-rendering the top bar can
 * keep the Smiley surprised while the press is still held (issue #50). */
let boardPressed = false;

/** Sends an Action through the controller and renders the fresh state. Only
 * the latest action's result is ever rendered — stale responses are dropped
 * by the controller (see createActionController). */
async function applyAndRender(action: Action): Promise<void> {
  const next = await controller.apply(action);
  if (next) {
    const prev = state;
    state = next;
    renderBoard(state, boardEl);
    renderTopBar(state);
    // Re-assert the gesture-driven face: a response re-rendering the top bar
    // must not wipe the surprise while a press is still held (issue #50).
    renderSmiley(state);
    // The game ended with this response: cancel any in-progress gesture so
    // no press-set or Chord Preview survives onto the Won/Lost board (#50).
    if (prev && isGameEnded(next.game_state) && !isGameEnded(prev.game_state)) {
      dispatchGesture({ kind: "game-ended" });
    }
  }
}

/** Feeds a gesture event to the machine and applies its output. Phase
 * changes and in-phase effects are traced at `debug` so gesture problems
 * are diagnosable from the console alone (the machine itself stays pure). */
function dispatchGesture(event: GestureEvent): void {
  // The event listeners are registered only after the initial state load, so
  // `state` is always present when a gesture is dispatched.
  const out = gesture.handle(event, state!.game_state);
  if (out.phaseChange) {
    log.debug(`gesture ${out.phaseChange}`, {
      event: event.kind,
      ...(out.action ? { action: out.action } : {}),
    });
  }
  for (const effect of out.effects) {
    log.debug(`gesture ${effect}`, {
      event: event.kind,
      ...(out.action ? { action: out.action } : {}),
    });
  }
  // A Reveal/Chord keeps its highlight until the response re-renders the
  // Board, so the Cells do not flash back to Hidden mid round trip (#32).
  if (
    out.action &&
    (out.action.type === "reveal" || out.action.type === "chord")
  ) {
    previewLayer.retain();
  }
  previewLayer.render(out.chordPreview, out.pressPreview);
  boardPressed = out.boardPressed;
  renderSmiley(state!);
  if (out.action) {
    const action = out.action;
    void applyAndRender(action)
      .catch((err) => {
        const message = err instanceof Error ? err.message : err;
        log.error(`Action ${action.type} failed: ${message}`);
      })
      .finally(() => previewLayer.release());
  }
}

/** Polls the state once per second to drive the Timer; the counter and
 * smiley come from action responses, which are always fresher. */
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
 * scope plus whether it is a Revealed numeric Cell (the criterion for
 * showing the Chord Preview). */
function cellHit(state: GameState, pos: Pos): CellHit {
  return {
    pos,
    previewCells: chordPreviewCells(state, pos.row, pos.col),
    isNumericCell: isRevealedNumericCell(state, pos.row, pos.col),
    isRevealed:
      state.cells[pos.row * state.cols + pos.col]?.state === "revealed",
  };
}

/** Renders the Smiley Button's face: surprised while a press is held over
 * the Board, otherwise the state-driven face (issue #50). */
function renderSmiley(state: GameState): void {
  const smiley = document.getElementById("smiley")!;
  smiley.textContent = boardPressed ? SmileyFace.surprised : smileyFace(state);
}

function handleRightDown(ev: MouseEvent): void {
  const cell = cellAt(ev);
  if (cell) {
    ev.preventDefault();
    dispatchGesture({
      kind: "right-down",
      cell: state ? cellHit(state, cellPos(cell)) : null,
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
    cell: cellHit(state, cellPos(cell)),
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
    cell: pos ? cellHit(state, pos) : null,
  });
}

function onContextMenu(ev: Event): void {
  ev.preventDefault();
}

function onAppClick(ev: Event): void {
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
  if (target.closest("#smiley")) {
    // The Smiley Button restarts with the current difficulty.
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
    document.getElementById("app")!.addEventListener("click", onAppClick);
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
