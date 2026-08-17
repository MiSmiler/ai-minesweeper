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

/** Renders the top bar (flag counter, result banner, timer) from state. */
export function renderTopBar(state: GameState): void {
  const counter = document.getElementById("counter")!;
  const banner = document.getElementById("banner")!;
  const timer = document.getElementById("timer")!;

  counter.textContent = `🚩 ${state.flags_remaining}`;

  // Highlight the active difficulty button.
  document.querySelectorAll<HTMLElement>("[data-difficulty]").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.difficulty === state.difficulty);
  });

  if (state.game_state === "won") {
    banner.textContent = "WON";
    banner.className = "banner won";
  } else if (state.game_state === "lost") {
    banner.textContent = "LOST";
    banner.className = "banner lost";
  } else {
    banner.textContent = "";
    banner.className = "banner";
  }

  timer.textContent = formatTimer(state.elapsed_secs);
}

export function formatTimer(secs: number): string {
  const minutes = Math.floor(secs / 60);
  const seconds = secs % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function cellText(cell: { state: string; content: unknown }): string {
  if (cell.state === "flagged") return "🚩";
  if (cell.state === "revealed") {
    if (cell.content === "mine") return "💣";
    if (typeof cell.content === "number" && cell.content > 0) return String(cell.content);
  }
  return "";
}
