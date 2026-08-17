import { fetchState, postAction, type Action, type GameState } from "./api";
import { renderBoard } from "./render";
import "./style.css";

const app = document.getElementById("app")!;

let state: GameState | null = null;
// Guards against out-of-order responses: only the latest action's result
// is rendered, so a slow earlier response can never show stale state.
let seq = 0;

async function applyAction(action: Action): Promise<void> {
  const id = ++seq;
  const next = await postAction(action);
  if (id !== seq) return;
  state = next;
  renderBoard(state, app);
}

function onBoardClick(ev: MouseEvent): void {
  if (ev.button !== 0) return;
  const cell = (ev.target as HTMLElement).closest<HTMLElement>(".cell");
  if (!cell) return;
  const row = Number(cell.dataset.row);
  const col = Number(cell.dataset.col);
  void applyAction({ type: "reveal", row, col });
}

async function main(): Promise<void> {
  try {
    state = await fetchState();
    renderBoard(state, app);
    app.addEventListener("click", onBoardClick);
  } catch (err) {
    app.textContent = `Failed to load game: ${err instanceof Error ? err.message : err}`;
  }
}

main();
