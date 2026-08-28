import { type Action, type Position } from "../api";

/** A hit-tested Cell plus the Cells its Chord Preview would highlight
 * (computed by the caller from the game state). `isNumericCell` is the
 * criterion for showing the Chord Preview (a Revealed numeric Cell);
 * `isRevealed` is the weaker criterion for Arming (any Revealed Cell). */
export interface CellHit {
  pos: Position;
  previewCells: Position[];
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
  pos: Position;
  cells: Position[];
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
 * (flag/reveal/chord) or a Preview lifecycle update (press-preview-* for
 * the Press Preview, chord-preview-* for the Chord Preview). */
export type GestureEffect =
  | "flag"
  | "reveal"
  | "chord"
  | "press-preview-set"
  | "press-preview-moved"
  | "press-preview-cleared"
  | "chord-preview-set"
  | "chord-preview-moved"
  | "chord-preview-cleared";

/** The effect of a gesture event: at most one action to send to the
 * server, the Previews to render (`null` when none is shown), the phase
 * transition this event caused (`undefined` when the phase did not
 * change), and the in-phase effects performed (empty when nothing changed
 * observably). */
export interface GestureOutput {
  action?: Action;
  pressPreview: Position | null;
  chordPreview: ChordPreview | null;
  phaseChange?: GesturePhaseChange;
  effects: GestureEffect[];
  /** Whether a press is held over the Board — the caller renders the Smiley
   * Button's surprise face from it (issue #50). */
  boardPressed: boolean;
}

/** The machine's full state: the gesture `phase` — the control state the
 * transition table is defined over — plus the extended state (which
 * buttons are held, where each press went down for the Arm eligibility
 * rule, whether the pointer has left the Board since Arming, the Press
 * Preview position and the Chord Preview). Data changes alone are not
 * phase changes and never produce a `phaseChange`. */
interface MachineState {
  phase: GesturePhase;
  rightHeld: boolean;
  rightOnRevealed: boolean;
  leftOnRevealed: boolean;
  /** Whether the pointer has left the Board since the gesture Armed — once
   * true, the Chord Preview stays cleared for the rest of the gesture
   * (mirroring the Press Preview's terminal clear): re-entering the Board
   * does not bring it back until the gesture re-arms. */
  pointerLeftBoard: boolean;
  pressPreview: Position | null;
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
  pointerLeftBoard: false,
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
 * flagged-consistent neighbors) when a Preview is shown. Leaving the Board
 * terminally clears the Chord Preview — it does not return on re-entry
 * until the gesture re-arms — mirroring the Press Preview. A Flag fires
 * only on Right-down with no Left held, and releasing Right does not
 * disarm an armed Chord. Pure: no DOM, no I/O — the caller renders the
 * output and traces the reported state changes and effects. */
export function createGestureMachine() {
  let state: MachineState = initial();
  /** The enable gate: closed (false) once the game is Won or Lost — the
   * machine then ignores every event — and reopened on a new game. Driven
   * by the caller from the game state; kept outside the phase state so the
   * phase transitions never have to carry it. */
  let enabled = true;

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
        ? "chord-preview-set"
        : next === null
          ? "chord-preview-cleared"
          : "chord-preview-moved";
    return { chordPreview: next, effect };
  };

  /** Clears the Chord Preview, reporting the transition only when one was
   * shown. */
  const clearChordPreview = (): {
    chordPreview: ChordPreview | null;
    effect?: GestureEffect;
  } => {
    if (!state.chordPreview) return { chordPreview: null };
    return { chordPreview: null, effect: "chord-preview-cleared" };
  };

  /** The state after the Left gesture ends: back to `idle`, both previews
   * and the Left press data cleared, while a held Right press survives. */
  const toIdle = (): MachineState => ({
    phase: "idle",
    rightHeld: state.rightHeld,
    rightOnRevealed: state.rightOnRevealed,
    leftOnRevealed: false,
    pointerLeftBoard: false,
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
   * is now ready to solve. A fresh arm also resets the pointer-left latch:
   * a new Chord gesture starts with a recoverable Preview. */
  const arm = (cell: CellHit): GestureDecision => {
    const p = setChordPreview(cell);
    return {
      next: {
        phase: "armed",
        rightHeld: true,
        rightOnRevealed: true,
        leftOnRevealed: true,
        pointerLeftBoard: false,
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
          effects: ["press-preview-set"],
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
            effects: ["press-preview-cleared"],
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
          effects: ["press-preview-moved"],
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
          effects: ["press-preview-cleared"],
        };
      }
      case "blur": {
        const reset = resetToIdle();
        return {
          next: reset.state,
          phaseChange: reset.phaseChange,
          effects: reset.effects,
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
          effects: action ? ["chord", "chord-preview-cleared"] : [],
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
        // The pointer left the Board while armed: the Chord Preview is
        // terminally cleared (like the Press Preview) — re-entering the
        // Board does not restore it until the gesture re-arms (ADR-0008).
        if (state.pointerLeftBoard) return {};
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
        // The latch is set even when no Preview was shown, so re-entering
        // over a previewable Cell cannot restore it.
        return {
          next: {
            ...state,
            pointerLeftBoard: true,
            chordPreview: p.chordPreview,
          },
          effects: p.effect ? [p.effect] : [],
        };
      }
      case "blur": {
        const reset = resetToIdle();
        return {
          next: reset.state,
          phaseChange: reset.phaseChange,
          effects: reset.effects,
        };
      }
      default:
        return {};
    }
  };

  /** Resets the machine to idle, reporting what the gesture was doing: the
   * phase change it was in and the Previews it clears. Shared by blur (the
   * window losing focus cancels the gesture) and setEnabled(false) (the
   * game ending mid-gesture cancels it, issue #50). The next state is
   * always `initial()`, so it is returned directly — never optional. */
  const resetToIdle = (): {
    state: MachineState;
    phaseChange?: GesturePhaseChange;
    effects: GestureEffect[];
  } => {
    const effects: GestureEffect[] = [];
    if (state.pressPreview) effects.push("press-preview-cleared");
    if (state.chordPreview) effects.push("chord-preview-cleared");
    return {
      state: initial(),
      phaseChange:
        state.phase === "pressing"
          ? "released"
          : state.phase === "armed"
            ? "disarmed"
            : undefined,
      effects,
    };
  };

  /** Decides the event's effect in the current phase. A closed machine (the
   * game Won or Lost, issue #50) is handled by `handle` before `decide` is
   * reached — the gate lives outside the phase table. */
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

  /** The machine's current renderable output when nothing changed. */
  const currentOutput = (): GestureOutput => ({
    pressPreview: state.pressPreview,
    chordPreview: state.chordPreview,
    effects: [],
    boardPressed: state.phase !== "idle" || state.rightHeld,
  });

  return {
    handle(event: GestureEvent): GestureOutput {
      // A closed machine reports nothing and ignores every event.
      if (!enabled) return currentOutput();
      const d = decide(event);
      if (d.next) state = d.next;
      return {
        action: d.action,
        pressPreview: state.pressPreview,
        chordPreview: state.chordPreview,
        phaseChange: d.phaseChange,
        effects: d.effects ?? [],
        boardPressed: state.phase !== "idle" || state.rightHeld,
      };
    },
    /** Closes or reopens the machine. Disabling cancels any in-progress
     * gesture (reported like a blur: the phase it was in, plus the Previews
     * it clears) and makes every event ignored; enabling restores handling.
     * The caller drives the gate from the game state on every response so it
     * cannot drift — idempotent when unchanged (issue #50). */
    setEnabled(value: boolean): GestureOutput {
      if (value === enabled) return currentOutput();
      if (value) {
        enabled = true;
        return currentOutput();
      }
      const reset = resetToIdle();
      state = reset.state;
      enabled = false;
      return {
        pressPreview: null,
        chordPreview: null,
        phaseChange: reset.phaseChange,
        effects: reset.effects ?? [],
        boardPressed: false,
      };
    },
  };
}
