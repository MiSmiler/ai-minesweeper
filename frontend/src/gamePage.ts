import { type Action, type GameSnapshot, type Position } from "./api";
import { createGameClient } from "./client";
import { cellAtPoint, measureBoard, type BoardGeometry } from "./hitTest";
import { log } from "./log";
import type { TopBarEls } from "./render";

/** Everything the game-page bootstrap needs: the game frame's elements and
 * the state/action functions the client sends through. Mirrors
 * GameClientDeps — the bootstrap is a thin DOM adapter over the client
 * module (ADR-0007), adding the geometry, hit-testing, listener
 * registration, pointer-move throttling, and difficulty/Smiley click
 * delegation the client does not own. */
export interface GamePageDeps {
  boardEl: HTMLElement;
  topBarEls: TopBarEls;
  post: (action: Action) => Promise<GameSnapshot>;
  fetchState: () => Promise<GameSnapshot>;
}

/** A running game page: the bootstrap has fetched the initial state and
 * registered every input listener. */
export interface GamePage {
  /** Fetches and renders the initial state, then registers all input
   * listeners. On failure it logs and shows the message in the Board (the
   * page is left dead); it never rejects. */
  start(): Promise<void>;
  /** Starts a new game; an optional difficulty switches to it (the Smiley
   * Button path keeps the current one). Kept on the bootstrap so a page can
   * drive a fresh game of a chosen difficulty (e.g. on navigation). */
  newGame(difficulty?: GameSnapshot["difficulty"]): void;
}

/** Bootstraps a Minesweeper game page from its game frame's elements and
 * the backend adapters. The human page and any future page (the ai page)
 * reuse the same bootstrap, so only the element lookups are page-specific —
 * none of the input wiring, hit-testing, or geometry caching is duplicated.
 *
 * The caller fetches the elements it wants to render into (a container
 * defined by its own markup) and hands them in; the bootstrap owns the client
 * module, registers every Board/window input listener, and delegates
 * difficulty and Smiley clicks. */
export function startGamePage(deps: GamePageDeps): GamePage {
  const { boardEl, topBarEls, post, fetchState } = deps;

  const client = createGameClient({ boardEl, topBarEls, post, fetchState });

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

  function handleLeftDown(ev: MouseEvent): void {
    const pos = posAt(ev);
    if (!pos) return;
    ev.preventDefault();
    client.handleInput({ kind: "left-down", pos });
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

  function onDifficultyClick(ev: Event): void {
    const target = ev.target as HTMLElement;
    const difficultyBtn = target.closest<HTMLElement>("[data-difficulty]");
    if (difficultyBtn) {
      // A difficulty button starts a fresh game of that difficulty.
      client.newGame(
        difficultyBtn.dataset.difficulty as GameSnapshot["difficulty"],
      );
    }
  }

  const start = async (): Promise<void> => {
    try {
      await client.init();
      boardEl.addEventListener("mousedown", onBoardMouseDown);
      boardEl.addEventListener("pointermove", onBoardPointerMove);
      boardEl.addEventListener("pointerleave", onBoardPointerLeave);
      boardEl.addEventListener("contextmenu", onContextMenu);
      // The Smiley Button restarts with the current difficulty; a difficulty
      // button starts a fresh game of that difficulty. Bound to the game
      // frame's own elements (no page-root delegation), so a page reusing the
      // bootstrap needs only its own top-bar/difficulty markup.
      topBarEls.smiley.addEventListener("click", () => client.newGame());
      topBarEls.difficultyRow.addEventListener("click", onDifficultyClick);
      window.addEventListener("mouseup", onWindowMouseUp);
      window.addEventListener("blur", onWindowBlur);
      window.setInterval(() => void client.pollTimer(), 1000);
    } catch (err) {
      const message = err instanceof Error ? err.message : err;
      log.error(`Failed to load game: ${message}`);
      boardEl.textContent = `Failed to load game: ${message}`;
    }
  };

  const newGame = (difficulty?: GameSnapshot["difficulty"]): void =>
    client.newGame(difficulty);

  return { start, newGame };
}
