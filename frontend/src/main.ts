import { fetchState, postAction, type Action, type GameState } from "./api";
import { formatTimer, renderBoard, renderTopBar } from "./render";
import "./style.css";

const boardEl = document.getElementById("board")!;

let state: GameState | null = null;
// Guards against out-of-order responses: only the latest action's result
// is rendered, so a slow earlier response can never show stale state.
let seq = 0;

// Chord gesture state: pressing Left while Right is held triggers a Chord
// instead of a Reveal (ADR-0003).
let rightHeld = false;

async function applyAction(action: Action): Promise<void> {
  const id = ++seq;
  const next = await postAction(action);
  if (id !== seq) return;
  state = next;
  renderBoard(state, boardEl);
  renderTopBar(state);
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

function onBoardMouseDown(ev: MouseEvent): void {
  if (ev.button === 2) {
    handleRightDown(ev);
  } else if (ev.button === 0) {
    handleLeftDown(ev);
  }
}

function handleRightDown(ev: MouseEvent): void {
  // The press is remembered for the chord gesture. The gesture starts on
  // any Right press within the Board (the mousedown listener lives on the
  // Board element), so even Right presses on non-Cell areas start it.
  rightHeld = true;
  const cell = cellAt(ev);
  if (!cell) return;
  ev.preventDefault();
  // Right-click toggles a Flag on Hidden Cells; the server ignores
  // Revealed Cells.
  const row = Number(cell.dataset.row);
  const col = Number(cell.dataset.col);
  void applyAction({ type: "flag", row, col });
}

function handleLeftDown(ev: MouseEvent): void {
  const cell = cellAt(ev);
  if (!cell) return;
  ev.preventDefault();
  const row = Number(cell.dataset.row);
  const col = Number(cell.dataset.col);
  if (rightHeld) {
    // Chord: Reveals the unflagged neighbors of a Revealed numeric Cell;
    // a no-op anywhere else (Hidden Cells, zero Cells, mismatched counts).
    void applyAction({ type: "chord", row, col });
  } else {
    void applyAction({ type: "reveal", row, col });
  }
}

function onWindowMouseUp(ev: MouseEvent): void {
  if (ev.button === 2) rightHeld = false;
}

function onWindowBlur(): void {
  rightHeld = false;
}

function onContextMenu(ev: Event): void {
  ev.preventDefault();
}

function onTopBarClick(ev: Event): void {
  const target = ev.target as HTMLElement;
  const difficultyBtn = target.closest<HTMLElement>("[data-difficulty]");
  if (difficultyBtn) {
    // A difficulty button starts a fresh game of that difficulty.
    void applyAction({
      type: "new-game",
      difficulty: difficultyBtn.dataset.difficulty as GameState["difficulty"],
    });
    return;
  }
  if (target.closest("#new-game")) {
    // New Game restarts with the current difficulty.
    void applyAction({ type: "new-game" });
  }
}

async function main(): Promise<void> {
  try {
    state = await fetchState();
    renderBoard(state, boardEl);
    renderTopBar(state);
    boardEl.addEventListener("mousedown", onBoardMouseDown);
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
