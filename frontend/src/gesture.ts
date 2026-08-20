import type { Action, Pos } from "./api";

/** A hit-tested Cell plus the Cells its Chord Preview would highlight
 * (computed by the caller from the game state), and whether it is a Revealed
 * numeric Cell — the arming criterion for the Chord, independent of how many
 * neighbors the Preview would highlight. */
export interface ChordTarget {
  pos: Pos;
  previewCells: Pos[];
  isNumericCell: boolean;
}

/** Abstract player-input events fed to the chord gesture machine. The DOM
 * layer translates mouse events into these: every event carries the
 * hit-tested Cell (with its preview Cells) so the machine stays pure — the
 * `right-down` payload lets it tell whether Right was pressed on a Revealed
 * numeric Cell, which is the arming requirement. */
export type GestureEvent =
  | { kind: "right-down"; cell: ChordTarget | null }
  | { kind: "left-down"; cell: ChordTarget | null }
  | { kind: "pointer-move"; cell: ChordTarget | null }
  | { kind: "left-up" }
  | { kind: "right-up" }
  | { kind: "blur" }
  | { kind: "pointer-leave" };

/** The transient highlight shown while the Chord gesture is armed: the Cell
 * the Chord would be applied to and the Cells it would Reveal. */
export interface ChordPreview {
  pos: Pos;
  cells: Pos[];
}

/** A state transition the machine made while handling an event, reported so
 * the caller can trace the gesture without the machine doing I/O itself
 * (the machine stays pure — see createGestureMachine). */
export type GestureTransition =
  | "armed"
  | "disarmed"
  | "preview-set"
  | "preview-moved"
  | "preview-cleared"
  | "chord"
  | "flag"
  | "reveal";

/** The effect of a gesture event: at most one Action to send to the server,
 * the Chord Preview state to render (`null` when nothing is armed), and the
 * state transition this event caused (`undefined` when nothing changed
 * observably). */
export interface GestureOutput {
  action?: Action;
  preview: ChordPreview | null;
  transition?: GestureTransition;
}

/** The chord gesture state machine (ADR-0003): holding Left and Right
 * together arms a Chord — in any order, wherever Right went down — shows its
 * Chord Preview while the pointer is over a Revealed numeric Cell, and
 * solves (Reveals the flagged-consistent neighbors) when Left is released.
 * Once armed the Preview is driven by Left alone: releasing Right does not
 * clear it. Pure: no DOM, no I/O — the caller renders the output and traces
 * the reported transitions. */
export function createGestureMachine() {
  let rightHeld = false;
  let leftHeld = false;
  let chordActive = false;
  let preview: ChordPreview | null = null;

  /** True when two Previews name the same Cell and the same highlight set. */
  const samePreview = (
    a: ChordPreview | null,
    b: ChordPreview | null,
  ): boolean => {
    if (a === null || b === null) return a === b;
    if (a.pos.row !== b.pos.row || a.pos.col !== b.pos.col) return false;
    if (a.cells.length !== b.cells.length) return false;
    return a.cells.every(
      (p, i) => p.row === b.cells[i].row && p.col === b.cells[i].col,
    );
  };

  /** Clears the Preview, reporting the transition only when one was shown. */
  const clearPreview = (): GestureOutput => {
    const transition = preview ? "preview-cleared" : undefined;
    preview = null;
    return transition ? { preview: null, transition } : { preview: null };
  };

  /** Shows the Preview over the given Cell's neighbors, or clears it when the
   * Cell has none to highlight. Callers have already confirmed isNumericCell.
   * Reports a transition only when the Preview actually changed. */
  const setPreview = (cell: ChordTarget): GestureOutput => {
    const next: ChordPreview | null =
      cell.previewCells.length > 0
        ? { pos: cell.pos, cells: cell.previewCells }
        : null;
    if (samePreview(preview, next)) return { preview };
    const transition: GestureTransition =
      preview === null
        ? "preview-set"
        : next === null
          ? "preview-cleared"
          : "preview-moved";
    preview = next;
    return { preview, transition };
  };

  return {
    handle(event: GestureEvent): GestureOutput {
      switch (event.kind) {
        case "right-down": {
          // The press is remembered for the chord gesture, even off a Cell.
          // A press on a Cell Flags it immediately; the server ignores
          // Revealed Cells. Holding Left too arms the Chord regardless of
          // where Right went down (see left-down).
          rightHeld = true;
          const armed = leftHeld;
          if (armed) {
            chordActive = true;
          }
          const out: GestureOutput = { preview };
          if (event.cell) {
            out.action = {
              type: "flag",
              row: event.cell.pos.row,
              col: event.cell.pos.col,
            };
          }
          if (armed) {
            out.transition = "armed";
          } else if (event.cell) {
            out.transition = "flag";
          }
          return out;
        }
        case "left-down": {
          if (!event.cell) {
            return { preview };
          }
          leftHeld = true;
          if (!rightHeld) {
            return {
              action: {
                type: "reveal",
                row: event.cell.pos.row,
                col: event.cell.pos.col,
              },
              preview,
              transition: "reveal",
            };
          }
          // Both buttons are held: arm the Chord. No action is sent yet —
          // the Preview follows the pointer and the Chord solves on left-up.
          chordActive = true;
          return { ...setPreview(event.cell), transition: "armed" };
        }
        case "pointer-move": {
          if (!chordActive) {
            return { preview };
          }
          if (!event.cell || !event.cell.isNumericCell) {
            return clearPreview();
          }
          return setPreview(event.cell);
        }
        case "left-up": {
          leftHeld = false;
          chordActive = false;
          if (preview) {
            const pos = preview.pos;
            preview = null;
            return {
              action: { type: "chord", row: pos.row, col: pos.col },
              preview: null,
              transition: "chord",
            };
          }
          return { preview, transition: "disarmed" };
        }
        case "right-up": {
          // Releasing Right does not disarm a Chord in progress: once armed,
          // the Preview is driven by Left alone.
          rightHeld = false;
          return { preview };
        }
        case "blur":
          rightHeld = false;
          leftHeld = false;
          chordActive = false;
          return { ...clearPreview(), transition: "disarmed" };
        case "pointer-leave":
          // Leaving the Board clears the Preview but keeps the gesture
          // armed: re-entering and moving over a Revealed numeric Cell
          // restores it.
          return clearPreview();
      }
    },
  };
}
