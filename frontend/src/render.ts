import type { GameState } from "./api";

/** Renders the board grid from the server state. Pure function of state. */
export function renderBoard(state: GameState, container: HTMLElement): void {
  const board = document.createElement("div");
  board.className = "board";
  board.style.gridTemplateColumns = `repeat(${state.cols}, var(--cell-size))`;

  for (let i = 0; i < state.cells.length; i++) {
    const cell = state.cells[i];
    const row = Math.floor(i / state.cols);
    const col = i % state.cols;
    const el = document.createElement("div");
    el.className = "cell";
    el.dataset.row = String(row);
    el.dataset.col = String(col);
    el.textContent = cellText(cell);

    if (cell.state === "flagged") {
      el.classList.add("cell-flagged");
    } else if (cell.state === "revealed") {
      el.classList.add("cell-revealed");
      if (cell.content === "mine") {
        el.classList.add("cell-mine");
        if (state.trigger && state.trigger.row === row && state.trigger.col === col) {
          el.classList.add("cell-trigger");
        }
      } else if (typeof cell.content === "number" && cell.content > 0) {
        el.classList.add(`n${cell.content}`);
      }
    }
    board.appendChild(el);
  }

  container.replaceChildren(board);
}

function cellText(cell: { state: string; content: unknown }): string {
  if (cell.state === "flagged") return "🚩";
  if (cell.state === "revealed") {
    if (cell.content === "mine") return "💣";
    if (typeof cell.content === "number" && cell.content > 0) return String(cell.content);
  }
  return "";
}
