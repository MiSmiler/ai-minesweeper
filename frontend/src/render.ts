import type { CellView, GameState } from "./api";

/** The Smiley Button's emoji faces, keyed by use. The state-driven face comes
 * from renderTopBar; the surprised face is set directly while pressing. */
export const SmileyFace = {
  neutral: "🙂",
  surprised: "😮",
  won: "😎",
  lost: "😭",
} as const;

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
        if (
          state.trigger &&
          state.trigger.row === row &&
          state.trigger.col === col
        ) {
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

/** The Smiley Button's state-driven face for a game state — the surprised
 * face while pressing is rendered by the caller from the gesture machine's
 * output (issue #50). */
export function smileyFace(state: GameState): SmileyFaceValue {
  if (state.game_state === "won") return SmileyFace.won;
  if (state.game_state === "lost") return SmileyFace.lost;
  return SmileyFace.neutral;
}

/** The face values of the Smiley Button. */
export type SmileyFaceValue = (typeof SmileyFace)[keyof typeof SmileyFace];

/** The elements the top-bar renderer writes into. */
export interface TopBarEls {
  counter: HTMLElement;
  smiley: HTMLElement;
  timer: HTMLElement;
  /** The row holding the difficulty buttons; the active one is highlighted. */
  difficultyRow: HTMLElement;
}

/** Renders the top bar (flag counter, smiley button, timer) from state into
 * the given elements, highlighting the active difficulty button in the row. */
export function renderTopBar(state: GameState, els: TopBarEls): void {
  els.counter.textContent = formatCounter(state.flags_remaining);
  els.smiley.textContent = smileyFace(state);

  // Highlight the active difficulty button.
  els.difficultyRow
    .querySelectorAll<HTMLElement>("[data-difficulty]")
    .forEach((btn) => {
      btn.classList.toggle(
        "active",
        btn.dataset.difficulty === state.difficulty,
      );
    });

  els.timer.textContent = formatTimer(state.elapsed_secs);
}

/** Formats elapsed seconds as a three-digit display, capped at 999 like the
 * classic Timer. */
export function formatTimer(secs: number): string {
  return String(Math.min(secs, 999)).padStart(3, "0");
}

/** Formats Flags Remaining for the three-digit counter: non-negative values
 * padded to three digits, negatives as minus sign plus digits. */
export function formatCounter(flags: number): string {
  return flags >= 0 ? String(flags).padStart(3, "0") : `-${Math.abs(flags)}`;
}

function cellText(cell: CellView): string {
  if (cell.state === "flagged") return "🚩";
  if (cell.state === "revealed") {
    if (cell.content === "mine") return "💣";
    if (typeof cell.content === "number" && cell.content > 0)
      return String(cell.content);
  }
  return "";
}
