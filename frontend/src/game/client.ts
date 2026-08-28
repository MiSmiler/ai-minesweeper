import {
  isGameEnded,
  type Action,
  type GameSnapshot,
  type Position,
} from "./api";
import { chordPreview } from "./logic/preview";
import {
  createGestureMachine,
  type CellHit,
  type GestureEvent,
  type GestureOutput,
} from "./logic/gesture";
import { log } from "../infra/log";
import { createPreviewRenderer } from "./render/previewRender";
import {
  formatTimer,
  renderBoard,
  renderTopBar,
  SmileyFace,
  smileyFace,
  type TopBarEls,
} from "./render/snapshotRender";

/** Abstract player input for the client module: the DOM adapter translates
 * mouse events into these — a kind plus the Cell under the pointer (or none
 * off-Board). The module builds the gesture machine's hit payload from its
 * cached snapshot. */
export type ClientInput =
  | { kind: "right-down"; pos: Position | null }
  | { kind: "left-down"; pos: Position | null }
  | { kind: "pointer-move"; pos: Position | null }
  | { kind: "left-up" }
  | { kind: "right-up" }
  | { kind: "blur" }
  | { kind: "pointer-leave" };

/** The dependencies the client module needs at creation: the elements it
 * renders into and the snapshot/action functions it sends through — the real
 * HTTP adapters in the browser, mocks in tests. */
export interface GameClientDeps {
  boardEl: HTMLElement;
  topBarEls: TopBarEls;
  post: (action: Action) => Promise<GameSnapshot>;
  fetchSnapshot: () => Promise<GameSnapshot>;
}

/** The client module: the frontend's view of the game. It owns the cached
 * snapshot, the gesture machine, the latest-action gate, the preview highlight
 * layer, and the Smiley Button, and renders everything — input arrives as
 * abstract events and the module decides what to send and what to show. */
export interface GameClient {
  /** Fetches and renders the initial snapshot. Rejects on failure. */
  init(): Promise<void>;
  /** Feeds an abstract input event through the gesture machine, renders the
   * resulting Previews and Smiley, and sends any action it decides on. */
  handleInput(event: ClientInput): void;
  /** Starts a new game; an optional difficulty switches to it (the Smiley
   * Button path keeps the current one). */
  newGame(difficulty?: GameSnapshot["difficulty"]): void;
  /** Refreshes the Timer from the server; the counter and Smiley come from
   * action responses, which are always fresher. */
  pollTimer(): Promise<void>;
}

export function createGameClient(deps: GameClientDeps): GameClient {
  const { boardEl, topBarEls, post, fetchSnapshot } = deps;

  const gesture = createGestureMachine();
  const previewRenderer = createPreviewRenderer(boardEl);

  let snapshot: GameSnapshot | null = null;

  /** Applies an Action through `post`, dropping the stale response of a
   * superseded one: the latest call wins, so a slow earlier response can
   * never render a stale snapshot. */
  let seq = 0;
  const applyLatest = async (action: Action): Promise<GameSnapshot | null> => {
    const id = ++seq;
    const next = await post(action);
    return id === seq ? next : null;
  };

  /** Whether a press is held over the Board, as reported by the last gesture
   * dispatch — kept so an action response re-rendering the top bar can keep
   * the Smiley surprised while the press is still held. */
  let boardPressed = false;

  /** Builds the hit-test payload for the gesture machine: the Chord Preview
   * the Cell would show (or null when it has no scope) plus whether it is
   * Revealed (the criterion for Arming). */
  const cellHit = (snapshot: GameSnapshot, pos: Position): CellHit => ({
    pos,
    chordPreview: chordPreview(snapshot, pos),
    isRevealed:
      snapshot.cells[pos.row * snapshot.cols + pos.col]?.state === "revealed",
  });

  /** Renders the Smiley Button's face: surprised while a press is held over
   * the Board, otherwise the state-driven face. */
  const renderSmiley = (snapshot: GameSnapshot): void => {
    topBarEls.smiley.textContent = boardPressed
      ? SmileyFace.surprised
      : smileyFace(snapshot);
  };

  /** Applies an action through the latest-action gate and renders the fresh
   * snapshot. Only the latest action's result is ever rendered — stale
   * responses are dropped. When the response ends the game, cancels any
   * in-progress gesture so no press-preview-set or Chord Preview survives onto the
   * Won/Lost board. */
  const applyAndRender = async (action: Action): Promise<void> => {
    const next = await applyLatest(action);
    if (next) {
      snapshot = next;
      // The machine's gate mirrors the game state: a Won/Lost response
      // closes it (cancelling any in-progress gesture so no press-preview-set or
      // Chord Preview survives onto the Won/Lost board, issue #50); any
      // other response leaves it open — idempotent, so in-progress gestures
      // survive non-ending responses.
      const gated = gesture.setEnabled(!isGameEnded(next.game_state));
      traceGesture(gated, "game-ended");
      boardPressed = gated.boardPressed;
      renderBoard(snapshot, boardEl);
      renderTopBar(snapshot, topBarEls);
      // Re-assert the gesture-driven face: a response re-rendering the top
      // bar must not wipe the surprise while a press is still held.
      renderSmiley(snapshot);
    }
  };

  /** Sends an action, logging failures and releasing any retained preview
   * highlight once the round trip settles (a no-op when nothing was
   * retained, e.g. a Flag or a new game). */
  const sendAction = (action: Action): void => {
    void applyAndRender(action)
      .catch((err) => {
        const message = err instanceof Error ? err.message : err;
        log.error(`Action ${action.type} failed: ${message}`);
      })
      .finally(() => previewRenderer.release());
  };

  /** Traces a gesture output's phase change and in-phase effects at
   * `debug` so gesture problems are diagnosable from the console alone (the
   * machine itself stays pure). */
  const traceGesture = (out: GestureOutput, eventKind: string): void => {
    if (out.phaseChange) {
      log.debug(`gesture ${out.phaseChange}`, {
        event: eventKind,
        ...(out.action ? { action: out.action } : {}),
      });
    }
    for (const effect of out.effects) {
      log.debug(`gesture ${effect}`, {
        event: eventKind,
        ...(out.action ? { action: out.action } : {}),
      });
    }
  };

  /** Feeds a gesture event to the machine and applies its output. Phase
   * changes and in-phase effects are traced at `debug` so gesture problems
   * are diagnosable from the console alone (the machine itself stays pure). */
  const dispatchGesture = (event: GestureEvent): void => {
    // The listeners are registered only after the initial snapshot load, so
    // `snapshot` is always present when a gesture is dispatched.
    const out = gesture.handle(event);
    traceGesture(out, event.kind);
    // A Reveal/Chord keeps its highlight until the response re-renders the
    // Board, so the Cells do not flash back to Hidden mid round trip.
    if (
      out.action &&
      (out.action.type === "reveal" || out.action.type === "chord")
    ) {
      previewRenderer.retain();
    }
    previewRenderer.render(out.preview);
    boardPressed = out.boardPressed;
    renderSmiley(snapshot!);
    if (out.action) {
      sendAction(out.action);
    }
  };

  /** Translates an abstract input event into a gesture-machine event,
   * building the hit payload from the cached snapshot. */
  const handleInput = (event: ClientInput): void => {
    if (snapshot === null) return;
    switch (event.kind) {
      case "right-down":
      case "left-down":
      case "pointer-move": {
        dispatchGesture({
          kind: event.kind,
          cell: event.pos ? cellHit(snapshot, event.pos) : null,
        });
        return;
      }
      case "left-up":
      case "right-up":
      case "blur":
      case "pointer-leave": {
        dispatchGesture({ kind: event.kind });
        return;
      }
    }
  };

  const newGame = (difficulty?: GameSnapshot["difficulty"]): void => {
    sendAction(
      difficulty ? { type: "new-game", difficulty } : { type: "new-game" },
    );
  };

  const pollTimer = async (): Promise<void> => {
    try {
      const next = await fetchSnapshot();
      topBarEls.timer.textContent = formatTimer(next.elapsed_secs);
    } catch {
      // Transient network errors are ignored; the next poll retries.
    }
  };

  const init = async (): Promise<void> => {
    snapshot = await fetchSnapshot();
    renderBoard(snapshot, boardEl);
    renderTopBar(snapshot, topBarEls);
  };

  return { init, handleInput, newGame, pollTimer };
}
