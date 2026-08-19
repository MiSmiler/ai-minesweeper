import { fetchState, postAction, type Action, type GameState, type Pos } from "./api";
import { chordPreviewCells } from "./chordPreview";
import { createActionController } from "./controller";
import { createGestureMachine, type GestureEvent, type GestureOutput } from "./gesture";
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

/** Feeds a gesture event to the machine and applies its output. */
function dispatchGesture(event: GestureEvent): void {
  const out = gesture.handle(event);
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

function onBoardMouseDown(ev: MouseEvent): void {
  if (ev.button === 2) {
    const cell = cellAt(ev);
    if (cell) {
      ev.preventDefault();
      dispatchGesture({ kind: "right-down", cell: cellPos(cell) });
    } else {
      // The press is still remembered for the chord gesture, even off a Cell.
      dispatchGesture({ kind: "right-down", cell: null });
    }
  } else if (ev.button === 0) {
    handleLeftDown(ev);
  }
}

function handleLeftDown(ev: MouseEvent): void {
  const cell = cellAt(ev);
  if (!cell || !state) return;
  ev.preventDefault();
  const pos = cellPos(cell);
  dispatchGesture({
    kind: "left-down",
    cell: { pos, previewCells: chordPreviewCells(state, pos.row, pos.col) },
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

function onBoardPointerLeave(): void {
  dispatchGesture({ kind: "pointer-leave" });
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
    boardEl.addEventListener("pointerleave", onBoardPointerLeave);
    boardEl.addEventListener("contextmenu", onContextMenu);
    document.querySelector(".top-bar")!.addEventListener("click", onTopBarClick);
    window.addEventListener("mouseup", onWindowMouseUp);
    window.addEventListener("blur", onWindowBlur);
    window.setInterval(() => void pollTimer(), 1000);
  } catch (err) {
    boardEl.textContent = `Failed to load game: ${err instanceof Error ? err.message : err}`;
  }
}

main();
