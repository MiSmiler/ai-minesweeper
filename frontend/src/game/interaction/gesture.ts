import { type Action, type Position } from "../api";
import { pressPreview, type Preview } from "./preview";

/** A hit-tested Cell plus the Chord Preview it would highlight (computed by the
 * caller from the game state). `chordPreview` is the criterion for showing the
 * Chord Preview (a Revealed numeric Cell's Hidden neighbors, or null when it has
 * no scope); `isRevealed` is the weaker criterion for Arming (any Revealed Cell). */
export interface CellHit {
  pos: Position;
  /** The Chord Preview over this Cell — always `kind: "chord"`, or null when the
   * Cell is not a Revealed numeric Cell, or has no Hidden neighbor scope. Never
   * the Press Preview (the machine builds that from `pos`). */
  chordPreview: Preview | null;
  /** Whether the press landed on a Revealed Cell — the Arm-eligibility
   * criterion, weaker than having a Chord Preview (any Revealed content counts). */
  isRevealed: boolean;
}

/** Abstract player-input events fed to the gesture machine. The DOM layer
 * translates mouse events into these: every event carries the hit-tested
 * Cell (with its Chord Preview) so the machine stays pure — the Cell's
 * `chordPreview` is the criterion for showing the Chord Preview and its
 * `isRevealed` flag the criterion for Arming. */
export type GestureEvent =
  | { kind: "right-down"; cell: CellHit | null }
  | { kind: "left-down"; cell: CellHit | null }
  | { kind: "pointer-move"; cell: CellHit | null }
  | { kind: "left-up" }
  | { kind: "right-up" }
  | { kind: "blur" }
  | { kind: "pointer-leave" };

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

/** The Preview lifecycle effect per kind and step — the press and chord
 * families stay split so tracing keeps the Press-vs-Chord distinction. */
const PREVIEW_EFFECT: Record<
  "press" | "chord",
  Record<"set" | "moved" | "cleared", GestureEffect>
> = {
  press: {
    set: "press-preview-set",
    moved: "press-preview-moved",
    cleared: "press-preview-cleared",
  },
  chord: {
    set: "chord-preview-set",
    moved: "chord-preview-moved",
    cleared: "chord-preview-cleared",
  },
};

/** The effect of a gesture event: at most one action to send to the
 * server, the Preview to render (`null` when none is shown), the phase
 * transition this event caused (`undefined` when the phase did not
 * change), and the in-phase effects performed (empty when nothing changed
 * observably). */
export interface GestureOutput {
  action?: Action;
  preview: Preview | null;
  phaseChange?: GesturePhaseChange;
  effects: GestureEffect[];
  /** Whether a press is held over the Board — the caller renders the Smiley
   * Button's surprise face from it (issue #50). */
  boardPressed: boolean;
}

/** The machine's full state: the gesture `phase` — the control state the
 * transition table is defined over — plus the extended state (which
 * buttons are held, where each press went down for the Arm eligibility
 * rule, whether the pointer has left the Board since Arming, and the one
 * active Preview). Data changes alone are not phase changes and never
 * produce a `phaseChange`. */
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
  preview: Preview | null;
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
  preview: null,
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

  /** True when two Previews name the same kind, Cell, and (for a Chord
   * Preview) highlight set. */
  const samePreview = (a: Preview | null, b: Preview | null): boolean => {
    if (a === null || b === null) return a === b;
    if (a.kind !== b.kind) return false;
    if (a.pos.row !== b.pos.row || a.pos.col !== b.pos.col) return false;
    if (a.kind === "chord" && b.kind === "chord") {
      if (a.cells.length !== b.cells.length) return false;
      return a.cells.every(
        (p, i) => p.row === b.cells[i].row && p.col === b.cells[i].col,
      );
    }
    return true;
  };

  /** The one active Preview, or null when none. Reports the preview
   * lifecycle effect only when the Preview actually changed — `set` when a
   * new kind takes over (or one appears), `moved` when the same kind
   * changes Cell, `cleared` when it goes away. */
  const setPreview = (
    next: Preview | null,
  ): { preview: Preview | null; effect?: GestureEffect } => {
    if (samePreview(state.preview, next)) {
      return { preview: state.preview };
    }
    if (next === null) {
      const kind = state.preview!.kind;
      return { preview: null, effect: PREVIEW_EFFECT[kind].cleared };
    }
    const effect =
      state.preview === null || state.preview.kind !== next.kind
        ? PREVIEW_EFFECT[next.kind].set
        : PREVIEW_EFFECT[next.kind].moved;
    return { preview: next, effect };
  };

  /** The state after the Left gesture ends: back to `idle`, the Preview and
   * the Left press data cleared, while a held Right press survives. */
  const toIdle = (): MachineState => ({
    phase: "idle",
    rightHeld: state.rightHeld,
    rightOnRevealed: state.rightOnRevealed,
    leftOnRevealed: false,
    pointerLeftBoard: false,
    preview: null,
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
    const p = setPreview(cell.chordPreview);
    return {
      next: {
        phase: "armed",
        rightHeld: true,
        rightOnRevealed: true,
        leftOnRevealed: true,
        pointerLeftBoard: false,
        preview: p.preview,
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
        const p = setPreview(pressPreview(event.cell.pos));
        return {
          next: {
            ...state,
            phase: "pressing",
            leftOnRevealed: event.cell.isRevealed,
            preview: p.preview,
          },
          phaseChange: "pressed",
          effects: p.effect ? [p.effect] : [],
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
        const pos = state.preview?.pos;
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
        if (!state.preview) return {};
        if (!event.cell) {
          const p = setPreview(null);
          return {
            next: {
              ...state,
              preview: p.preview,
              leftOnRevealed: false,
            },
            effects: p.effect ? [p.effect] : [],
          };
        }
        const next = pressPreview(event.cell.pos);
        if (samePreview(state.preview, next)) return {};
        const p = setPreview(next);
        return {
          next: { ...state, preview: p.preview },
          effects: p.effect ? [p.effect] : [],
        };
      }
      case "pointer-leave": {
        if (!state.preview) return {};
        const p = setPreview(null);
        return {
          next: {
            ...state,
            preview: p.preview,
            leftOnRevealed: false,
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

  const decideArmed = (event: GestureEvent): GestureDecision => {
    switch (event.kind) {
      case "left-up": {
        const pos = state.preview?.pos;
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
        const p = setPreview(!event.cell ? null : event.cell.chordPreview);
        if (p.preview === state.preview) return {};
        return {
          next: { ...state, preview: p.preview },
          effects: p.effect ? [p.effect] : [],
        };
      }
      case "pointer-leave": {
        const p = setPreview(null);
        // The latch is set even when no Preview was shown, so re-entering
        // over a previewable Cell cannot restore it.
        return {
          next: {
            ...state,
            pointerLeftBoard: true,
            preview: p.preview,
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
   * phase change it was in and the Preview it clears. Shared by blur (the
   * window losing focus cancels the gesture) and setEnabled(false) (the
   * game ending mid-gesture cancels it, issue #50). The next state is
   * always `initial()`, so it is returned directly — never optional. */
  const resetToIdle = (): {
    state: MachineState;
    phaseChange?: GesturePhaseChange;
    effects: GestureEffect[];
  } => {
    const effects: GestureEffect[] = [];
    if (state.preview) effects.push(`${state.preview.kind}-preview-cleared`);
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
    preview: state.preview,
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
        preview: state.preview,
        phaseChange: d.phaseChange,
        effects: d.effects ?? [],
        boardPressed: state.phase !== "idle" || state.rightHeld,
      };
    },
    /** Closes or reopens the machine. Disabling cancels any in-progress
     * gesture (reported like a blur: the phase it was in, plus the Preview it
     * clears) and makes every event ignored; enabling restores handling.
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
        preview: null,
        phaseChange: reset.phaseChange,
        effects: reset.effects ?? [],
        boardPressed: false,
      };
    },
  };
}
