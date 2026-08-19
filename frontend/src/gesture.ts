import type { Action, Pos } from "./api";

/** Abstract player-input events fed to the chord gesture machine. The DOM
 * layer translates mouse events into these: a `right-down` carries the
 * hit-tested Cell to Flag (if any), and a `left-down` carries the hit-tested
 * Cell plus the Cells its Chord Preview would highlight (computed by
 * chordPreviewCells). */
export type GestureEvent =
  | { kind: "right-down"; cell: Pos | null }
  | { kind: "left-down"; cell: { pos: Pos; previewCells: Pos[] } | null }
  | { kind: "left-up" }
  | { kind: "right-up" }
  | { kind: "blur" }
  | { kind: "pointer-leave" };

/** The effect of a gesture event: at most one Action to send to the server,
 * and the Chord Preview state to render (`null` when nothing is armed). */
export interface GestureOutput {
  action?: Action;
  preview: { pos: Pos; cells: Pos[] } | null;
}

/** The chord gesture state machine (ADR-0003): pressing Left while Right is
 * held arms a Chord, shows its Chord Preview while Left is held, and solves
 * (Reveals the flagged-consistent neighbors) when Left is released. Pure: no
 * DOM, no I/O — the caller renders the output. */
export function createGestureMachine() {
  let rightHeld = false;
  let preview: { pos: Pos; cells: Pos[] } | null = null;

  return {
    handle(event: GestureEvent): GestureOutput {
      switch (event.kind) {
        case "right-down": {
          // The press is remembered for the chord gesture, even off a Cell.
          // A press on a Cell Flags it immediately; the server ignores
          // Revealed Cells.
          rightHeld = true;
          const out: GestureOutput = { preview };
          if (event.cell) {
            out.action = { type: "flag", row: event.cell.row, col: event.cell.col };
          }
          return out;
        }
        case "left-down": {
          if (!event.cell) {
            return { preview };
          }
          if (!rightHeld) {
            return {
              action: { type: "reveal", row: event.cell.pos.row, col: event.cell.pos.col },
              preview,
            };
          }
          if (event.cell.previewCells.length > 0) {
            // Arming the Chord: show the Chord Preview over the Cells the
            // Chord would Reveal. No action is sent yet — the Chord solves
            // on left-up.
            preview = { pos: event.cell.pos, cells: event.cell.previewCells };
          }
          return { preview };
        }
        case "left-up": {
          if (preview) {
            const pos = preview.pos;
            preview = null;
            return { action: { type: "chord", row: pos.row, col: pos.col }, preview: null };
          }
          return { preview };
        }
        case "right-up":
          rightHeld = false;
          preview = null;
          return { preview: null };
        case "blur":
          rightHeld = false;
          preview = null;
          return { preview: null };
        case "pointer-leave":
          // Leaving the Board disarms the Preview but keeps Right held:
          // re-entering and pressing Left again re-arms it.
          preview = null;
          return { preview: null };
      }
    },
  };
}
