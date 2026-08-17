import { fetchState, postAction, type Action, type GameState } from "./api";
import { renderBoard } from "./render";
import "./style.css";

const app = document.getElementById("app")!;

let state: GameState | null = null;
// Guards against out-of-order responses: only the latest action's result
// is rendered, so a slow earlier response can never show stale state.
let seq = 0;

// Chord gesture state (matches the terminal): pressing Left while Right is
// held triggers a Chord instead of a Reveal (ADR-0003 keeps the gesture).
let rightHeld = false;

async function applyAction(action: Action): Promise<void> {
  const id = ++seq;
  const next = await postAction(action);
  if (id !== seq) return;
  state = next;
  renderBoard(state, app);
}

function cellAt(ev: MouseEvent): HTMLElement | null {
  return (ev.target as HTMLElement).closest<HTMLElement>(".cell");
}

function onBoardMouseDown(ev: MouseEvent): void {
  const cell = cellAt(ev);

  if (ev.button === 2) {
    // Right-click toggles a Flag on Hidden Cells; the server ignores
    // Revealed Cells. The press is also remembered for the chord gesture.
    rightHeld = true;
    if (!cell) return;
    ev.preventDefault();
    const row = Number(cell.dataset.row);
    const col = Number(cell.dataset.col);
    void applyAction({ type: "flag", row, col });
    return;
  }

  if (ev.button !== 0 || !cell) return;
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

async function main(): Promise<void> {
  try {
    state = await fetchState();
    renderBoard(state, app);
    app.addEventListener("mousedown", onBoardMouseDown);
    app.addEventListener("contextmenu", onContextMenu);
    window.addEventListener("mouseup", onWindowMouseUp);
    window.addEventListener("blur", onWindowBlur);
  } catch (err) {
    app.textContent = `Failed to load game: ${err instanceof Error ? err.message : err}`;
  }
}

main();
