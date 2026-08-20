import type { Action, Pos } from "./api";

/** A hit-tested Cell plus the Cells its Chord Preview would highlight
 * (computed by the caller from the game state). `isNumericCell` is the
 * criterion for showing the Chord Preview (a Revealed numeric Cell);
 * `isRevealed` is the weaker criterion for Arming (any Revealed Cell). */
export interface CellHit {
  pos: Pos;
  previewCells: Pos[];
  isNumericCell: boolean;
  /** Whether the press landed on a Revealed Cell — the Arm-eligibility
   * criterion, weaker than `isNumericCell` (any Revealed content counts). */
  isRevealed: boolean;
}

/** Abstract player-input events fed to the gesture machine. The DOM layer
 * translates mouse events into these: every event carries the hit-tested
 * Cell (with its preview Cells) so the machine stays pure — the Cell's
 * `isNumericCell` flag is the criterion for showing the Chord Preview and
 * its `isRevealed` flag the criterion for Arming. */
export type GestureEvent =
  | { kind: "right-down"; cell: CellHit | null }
  | { kind: "left-down"; cell: CellHit | null }
  | { kind: "pointer-move"; cell: CellHit | null }
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

/** The gesture machine's phase. `idle` — no gesture in progress (a Right
 * press may be held: its Flag was sent and the press is remembered for a
 * possible Chord). `pressing` — the Left press gesture: the Press Preview
 * follows the pointer and releasing Left Reveals. `armed` — the Chord
 * gesture: the Chord Preview follows the pointer and releasing Left
 * Chords. */
export type GesturePhase = "idle" | "pressing" | "armed";

/** A phase transition the machine made while handling an event — the
 * machine's control state is its `phase`, so a phase change is the state
 * change this event caused. Reported so the caller can trace the gesture
 * without the machine doing I/O itself (the machine stays pure — see
 * createGestureMachine). */
export type GesturePhaseChange = "pressed" | "released" | "armed" | "disarmed";

/** An effect the machine performed within a phase: a server action
 * (flag/reveal/chord) or a Preview lifecycle update (press-set and the
 * preview-set family). */
export type GestureEffect =
  | "flag"
  | "reveal"
  | "chord"
  | "press-set"
  | "press-moved"
  | "press-cleared"
  | "preview-set"
  | "preview-moved"
  | "preview-cleared";

/** The effect of a gesture event: at most one action to send to the
 * server, the Previews to render (`null` when none is shown), the phase
 * transition this event caused (`undefined` when the phase did not
 * change), and the in-phase effects performed (empty when nothing changed
 * observably). */
export interface GestureOutput {
  action?: Action;
  pressPreview: Pos | null;
  chordPreview: ChordPreview | null;
  phaseChange?: GesturePhaseChange;
  effects: GestureEffect[];
}

/** The machine's full state: the gesture `phase` — the control state the
 * transition table is defined over — plus the extended state (which
 * buttons are held, where each press went down for the Arm eligibility
 * rule, the Press Preview position and the Chord Preview). Data changes
 * alone are not phase changes and never produce a `phaseChange`. */
interface MachineState {
  phase: GesturePhase;
  rightHeld: boolean;
  rightOnRevealed: boolean;
  leftOnRevealed: boolean;
  pressPreview: Pos | null;
  chordPreview: ChordPreview | null;
}

/** What an event decided: the next state to commit (absent when nothing
 * changed) plus the observable deltas — the server action, the phase
 * change, and the in-phase effects. */
interface GestureDecision {
  next?: MachineState;
  action?: Action;
  phaseChange?: GesturePhaseChange;
  effects?: GestureEffect[];
}

const initial = (): MachineState => ({
  phase: "idle",
  rightHeld: false,
  rightOnRevealed: false,
  leftOnRevealed: false,
  pressPreview: null,
  chordPreview: null,
});

/** The gesture state machine: an explicit three-phase machine — idle,
 * pressing, armed. The Left press is a preview gesture: pressing shows a
 * Press Preview that follows the pointer, releasing Left Reveals the Cell
 * under the pointer, and leaving the Board cancels the press. Holding Left
 * and Right together Arms a Chord — in either press order — provided both
 * presses land on Revealed Cells; the Chord Preview follows the pointer
 * over Revealed numeric Cells, and releasing Left solves (Reveals the
 * flagged-consistent neighbors) when a Preview is shown. A Flag fires only
 * on Right-down with no Left held, and releasing Right does not disarm an
 * armed Chord. Pure: no DOM, no I/O — the caller renders the output and
 * traces the reported state changes and effects. */
export function createGestureMachine() {
  let state: MachineState = initial();

  /** True when two Chord Previews name the same Cell and highlight set. */
  const sameChordPreview = (
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

  /** The Chord Preview over `cell`, or null when it has no scope. Reports
   * the preview lifecycle effect only when the Preview actually changed. */
  const setChordPreview = (
    cell: CellHit,
  ): { chordPreview: ChordPreview | null; effect?: GestureEffect } => {
    const next: ChordPreview | null =
      cell.previewCells.length > 0
        ? { pos: cell.pos, cells: cell.previewCells }
        : null;
    if (sameChordPreview(state.chordPreview, next)) {
      return { chordPreview: state.chordPreview };
    }
    const effect: GestureEffect =
      state.chordPreview === null
        ? "preview-set"
        : next === null
          ? "preview-cleared"
          : "preview-moved";
    return { chordPreview: next, effect };
  };

  /** Clears the Chord Preview, reporting the transition only when one was
   * shown. */
  const clearChordPreview = (): {
    chordPreview: ChordPreview | null;
    effect?: GestureEffect;
  } => {
    if (!state.chordPreview) return { chordPreview: null };
    return { chordPreview: null, effect: "preview-cleared" };
  };

  /** The state after the Left gesture ends: back to `idle`, both previews
   * and the Left press data cleared, while a held Right press survives. */
  const toIdle = (): MachineState => ({
    phase: "idle",
    rightHeld: state.rightHeld,
    rightOnRevealed: state.rightOnRevealed,
    leftOnRevealed: false,
    pressPreview: null,
    chordPreview: null,
  });

  /** The state after a Right release: the Right press is forgotten and
   * nothing else changes — releasing Right never disarms an armed Chord. */
  const releaseRight = (): MachineState => ({
    ...state,
    rightHeld: false,
    rightOnRevealed: false,
  });

  /** Arms the Chord over `cell` — the second press of either order just
   * landed on a Revealed Cell and the first press did too, so the gesture
   * is now ready to solve. */
  const arm = (cell: CellHit): GestureDecision => {
    const p = setChordPreview(cell);
    return {
      next: {
        phase: "armed",
        rightHeld: true,
        rightOnRevealed: true,
        leftOnRevealed: true,
        pressPreview: null,
        chordPreview: p.chordPreview,
      },
      phaseChange: "armed",
      effects: p.effect ? [p.effect] : [],
    };
  };

  const decideIdle = (event: GestureEvent): GestureDecision => {
    switch (event.kind) {
      case "right-down": {
        const next: MachineState = {
          ...state,
          rightHeld: true,
          rightOnRevealed: event.cell?.isRevealed ?? false,
        };
        // A Flag only makes sense on a non-Revealed Cell (the server no-ops
        // it on Revealed Cells), so skip the round trip there: an arming
        // right-down sends nothing and no in-flight response can re-render
        // the Board mid-gesture and wipe an armed Chord's Preview.
        if (event.cell && !event.cell.isRevealed) {
          const action: Action = {
            type: "flag",
            row: event.cell.pos.row,
            col: event.cell.pos.col,
          };
          return { next, action, effects: ["flag"] };
        }
        return { next };
      }
      case "left-down": {
        if (!event.cell) return {};
        if (state.rightHeld && state.rightOnRevealed && event.cell.isRevealed) {
          return arm(event.cell);
        }
        return {
          next: {
            ...state,
            phase: "pressing",
            leftOnRevealed: event.cell.isRevealed,
            pressPreview: event.cell.pos,
          },
          phaseChange: "pressed",
          effects: ["press-set"],
        };
      }
      case "right-up":
        return { next: releaseRight() };
      default:
        return {};
    }
  };

  const decidePressing = (event: GestureEvent): GestureDecision => {
    switch (event.kind) {
      case "left-up": {
        const pos = state.pressPreview;
        const action: Action | undefined = pos
          ? { type: "reveal", row: pos.row, col: pos.col }
          : undefined;
        return {
          next: toIdle(),
          action,
          phaseChange: "released",
          effects: action ? ["reveal"] : [],
        };
      }
      case "right-down": {
        if (state.leftOnRevealed && event.cell?.isRevealed) {
          return arm(event.cell);
        }
        return {
          next: {
            ...state,
            rightHeld: true,
            rightOnRevealed: event.cell?.isRevealed ?? false,
          },
        };
      }
      case "right-up":
        return { next: releaseRight() };
      case "pointer-move": {
        if (!state.pressPreview) return {};
        if (!event.cell) {
          return {
            next: {
              ...state,
              pressPreview: null,
              leftOnRevealed: false,
            },
            effects: ["press-cleared"],
          };
        }
        if (
          event.cell.pos.row === state.pressPreview.row &&
          event.cell.pos.col === state.pressPreview.col
        ) {
          return {};
        }
        return {
          next: { ...state, pressPreview: event.cell.pos },
          effects: ["press-moved"],
        };
      }
      case "pointer-leave": {
        if (!state.pressPreview) return {};
        return {
          next: {
            ...state,
            pressPreview: null,
            leftOnRevealed: false,
          },
          effects: ["press-cleared"],
        };
      }
      case "blur": {
        return {
          next: initial(),
          phaseChange: "released",
          effects: state.pressPreview ? ["press-cleared"] : [],
        };
      }
      default:
        return {};
    }
  };

  const decideArmed = (event: GestureEvent): GestureDecision => {
    switch (event.kind) {
      case "left-up": {
        const pos = state.chordPreview?.pos;
        const action: Action | undefined = pos
          ? { type: "chord", row: pos.row, col: pos.col }
          : undefined;
        return {
          next: toIdle(),
          action,
          phaseChange: "disarmed",
          effects: action ? ["chord", "preview-cleared"] : [],
        };
      }
      case "right-up":
        return { next: releaseRight() };
      case "right-down":
        return {
          next: {
            ...state,
            rightHeld: true,
            rightOnRevealed: event.cell?.isRevealed ?? false,
          },
        };
      case "pointer-move": {
        const p =
          !event.cell || !event.cell.isNumericCell
            ? clearChordPreview()
            : setChordPreview(event.cell);
        if (p.chordPreview === state.chordPreview) return {};
        return {
          next: { ...state, chordPreview: p.chordPreview },
          effects: p.effect ? [p.effect] : [],
        };
      }
      case "pointer-leave": {
        const p = clearChordPreview();
        if (p.chordPreview === state.chordPreview) return {};
        return {
          next: { ...state, chordPreview: p.chordPreview },
          effects: p.effect ? [p.effect] : [],
        };
      }
      case "blur": {
        return {
          next: initial(),
          phaseChange: "disarmed",
          effects: state.chordPreview ? ["preview-cleared"] : [],
        };
      }
      default:
        return {};
    }
  };

  /** Decides the event's effect in the current phase. */
  const decide = (event: GestureEvent): GestureDecision => {
    switch (state.phase) {
      case "idle":
        return decideIdle(event);
      case "pressing":
        return decidePressing(event);
      case "armed":
        return decideArmed(event);
    }
  };

  return {
    handle(event: GestureEvent): GestureOutput {
      const d = decide(event);
      if (d.next) state = d.next;
      return {
        action: d.action,
        pressPreview: state.pressPreview,
        chordPreview: state.chordPreview,
        phaseChange: d.phaseChange,
        effects: d.effects ?? [],
      };
    },
  };
}
