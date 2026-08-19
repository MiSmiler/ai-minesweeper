import { describe, expect, it } from "vitest";
import type { Pos } from "./api";
import { createGestureMachine, type GestureEvent, type GestureOutput } from "./gesture";

const pos = (row: number, col: number): Pos => ({ row, col });

/** Runs a sequence of gesture events through a fresh machine, returning the
 * output of every event in order. */
function run(events: GestureEvent[]): GestureOutput[] {
  const machine = createGestureMachine();
  return events.map((event) => machine.handle(event));
}

const previewable = (row: number, col: number, previewCells: Pos[]) => ({
  pos: pos(row, col),
  previewCells,
});

describe("createGestureMachine", () => {
  it("sends a Flag action on right-down on a Cell, carrying its Position", () => {
    const [out] = run([{ kind: "right-down", cell: pos(1, 2) }]);
    expect(out.action).toEqual({ type: "flag", row: 1, col: 2 });
    expect(out.preview).toBeNull();
  });

  it("arms the gesture on right-down off a Cell without sending an action", () => {
    const out = run([
      { kind: "right-down", cell: null },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
    ]);
    expect(out[0].action).toBeUndefined();
    expect(out[1].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
  });

  it("sends a Reveal action on left-down without Right held, carrying the Position", () => {
    const [out] = run([
      { kind: "left-down", cell: previewable(3, 4, []) },
    ]);
    expect(out.action).toEqual({ type: "reveal", row: 3, col: 4 });
    expect(out.preview).toBeNull();
  });

  it("does nothing on left-down off a Cell", () => {
    const [out] = run([{ kind: "left-down", cell: null }]);
    expect(out.action).toBeUndefined();
    expect(out.preview).toBeNull();
  });

  it("arms the Chord Preview on left-down while Right is held on a previewable Cell, sending no action", () => {
    const cells = [pos(0, 0), pos(0, 1)];
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, cells) },
    ]);
    expect(out[1].action).toBeUndefined();
    expect(out[1].preview).toEqual({ pos: pos(1, 1), cells });
  });

  it("does not arm on left-down while Right is held on a non-previewable Cell", () => {
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, []) },
      // The gesture is still armed: a later previewable press arms it.
      { kind: "left-down", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[1].preview).toBeNull();
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
  });

  it("re-arms on a new previewable Cell while Right is held, moving the Preview", () => {
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "left-down", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[1].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
    expect(out[2].action).toBeUndefined();
  });

  it("sends a Chord action on left-up while armed and clears the Preview", () => {
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "left-up" },
    ]);
    expect(out[2].action).toEqual({ type: "chord", row: 1, col: 1 });
    expect(out[2].preview).toBeNull();
  });

  it("sends nothing on left-up when no Chord Preview is armed", () => {
    const [out] = run([{ kind: "left-up" }]);
    expect(out.action).toBeUndefined();
    expect(out.preview).toBeNull();
  });

  it("disarms the gesture on right-up, so a later left-up sends no Chord", () => {
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "right-up" },
      { kind: "left-up" },
    ]);
    expect(out[2].preview).toBeNull();
    expect(out[3].action).toBeUndefined();
  });

  it("disarms on blur, clearing the Preview and forgetting Right is held", () => {
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "blur" },
      { kind: "left-up" },
    ]);
    expect(out[2].preview).toBeNull();
    expect(out[3].action).toBeUndefined();
  });

  it("clears the Preview on pointer-leave but keeps Right held", () => {
    const out = run([
      { kind: "right-down", cell: pos(1, 1) },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-leave" },
      // Re-entering and pressing Left again re-arms; releasing solves.
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "left-up" },
    ]);
    expect(out[2].preview).toBeNull();
    expect(out[3].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
    expect(out[4].action).toEqual({ type: "chord", row: 1, col: 1 });
  });

  it("sends a Flag on every right-down on a Cell, even while a gesture is armed", () => {
    const out = run([
      { kind: "right-down", cell: pos(0, 0) },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "right-down", cell: pos(0, 1) },
    ]);
    expect(out[0].action).toEqual({ type: "flag", row: 0, col: 0 });
    expect(out[2].action).toEqual({ type: "flag", row: 0, col: 1 });
  });
});
