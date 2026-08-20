import { describe, expect, it } from "vitest";
import type { Pos } from "./api";
import { createGestureMachine, type ChordTarget, type GestureEvent, type GestureOutput } from "./gesture";

const pos = (row: number, col: number): Pos => ({ row, col });

/** Runs a sequence of gesture events through a fresh machine, returning the
 * output of every event in order. */
function run(events: GestureEvent[]): GestureOutput[] {
  const machine = createGestureMachine();
  return events.map((event) => machine.handle(event));
}

const previewable = (
  row: number,
  col: number,
  previewCells: Pos[],
  isNumericCell = previewCells.length > 0,
) => ({ pos: pos(row, col), previewCells, isNumericCell });

const rightDown = (cell: ChordTarget | null): GestureEvent => ({
  kind: "right-down",
  cell,
});

describe("createGestureMachine", () => {
  it("sends a Flag action on right-down on a Cell, carrying its Position", () => {
    const [out] = run([rightDown(previewable(1, 2, [pos(0, 1)]))]);
    expect(out.action).toEqual({ type: "flag", row: 1, col: 2 });
    expect(out.preview).toBeNull();
  });

  it("arms on left-down even when Right was pressed off a Cell, without sending an action", () => {
    const out = run([
      { kind: "right-down", cell: null },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
    ]);
    expect(out[0].action).toBeUndefined();
    expect(out[1].action).toBeUndefined();
    expect(out[1].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
  });

  it("sends a Reveal action on left-down without Right held, carrying the Position", () => {
    const [out] = run([{ kind: "left-down", cell: previewable(3, 4, []) }]);
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
      rightDown(previewable(1, 1, cells)),
      { kind: "left-down", cell: previewable(1, 1, cells) },
    ]);
    expect(out[1].action).toBeUndefined();
    expect(out[1].preview).toEqual({ pos: pos(1, 1), cells });
  });

  it("does not render on left-down over a non-previewable Cell, but a later previewable press arms it", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, []) },
      { kind: "left-down", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[1].preview).toBeNull();
    expect(out[1].action).toBeUndefined();
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
  });

  it("moves the Preview on pointer-move over a new previewable Cell while armed", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[1].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
    expect(out[2].action).toBeUndefined();
  });

  it("sends a Chord action on left-up while armed and clears the Preview", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
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

  it("keeps the Preview armed on right-up, so a later left-up still sends a Chord", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "right-up" },
      { kind: "left-up" },
    ]);
    expect(out[2].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
    expect(out[3].action).toEqual({ type: "chord", row: 1, col: 1 });
  });

  it("disarms on blur, clearing the Preview and forgetting the buttons", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "blur" },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
      { kind: "left-up" },
    ]);
    expect(out[2].preview).toBeNull();
    expect(out[3].preview).toBeNull();
    expect(out[4].action).toBeUndefined();
  });

  it("clears the Preview on pointer-leave but keeps the gesture armed", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-leave" },
      // Re-entering restores the Preview by moving; releasing solves.
      { kind: "pointer-move", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "left-up" },
    ]);
    expect(out[2].preview).toBeNull();
    expect(out[3].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
    expect(out[4].action).toEqual({ type: "chord", row: 1, col: 1 });
  });

  it("sends a Flag on every right-down on a Cell, even while a gesture is armed", () => {
    const out = run([
      rightDown(previewable(0, 0, [])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      rightDown(previewable(0, 1, [])),
    ]);
    expect(out[0].action).toEqual({ type: "flag", row: 0, col: 0 });
    expect(out[2].action).toEqual({ type: "flag", row: 0, col: 1 });
  });

  it("arms when Right was pressed on a non-previewable Cell, rendering on pointer-move over a previewable Cell", () => {
    const out = run([
      rightDown(previewable(1, 1, [])),
      { kind: "left-down", cell: previewable(1, 1, []) },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
      { kind: "left-up" },
    ]);
    expect(out[1].action).toBeUndefined();
    expect(out[1].preview).toBeNull();
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
    expect(out[3].action).toEqual({ type: "chord", row: 2, col: 2 });
  });

  it("does nothing on pointer-move when no gesture is armed", () => {
    const [out] = run([{ kind: "pointer-move", cell: previewable(1, 1, [pos(0, 0)]) }]);
    expect(out.action).toBeUndefined();
    expect(out.preview).toBeNull();
  });

  it("does nothing on pointer-move when Right is held but Left was never pressed", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[1].preview).toBeNull();
    expect(out[1].action).toBeUndefined();
  });

  it("clears the Preview on pointer-move over a non-previewable Cell while armed", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-move", cell: previewable(3, 3, []) },
    ]);
    expect(out[2].preview).toBeNull();
  });

  it("clears the Preview on pointer-move off a Cell while armed", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-move", cell: null },
    ]);
    expect(out[2].preview).toBeNull();
  });

  it("re-renders the Preview on pointer-move back over a previewable Cell after leaving", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-move", cell: previewable(3, 3, []) },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[2].preview).toBeNull();
    expect(out[3].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
  });

  it("keeps updating the Preview after right-up, driven by Left alone", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "right-up" },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[2].preview).toEqual({ pos: pos(1, 1), cells: [pos(0, 0)] });
    expect(out[3].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
  });

  it("sends no Chord on left-up when the Preview was cleared by moving off", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-move", cell: previewable(3, 3, []) },
      { kind: "left-up" },
    ]);
    expect(out[3].action).toBeUndefined();
  });

  it("arms on a Left-then-Right order, rendering on pointer-move over a previewable Cell", () => {
    const out = run([
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[0].action).toEqual({ type: "reveal", row: 1, col: 1 });
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
  });

  it("enters the gesture on left-down over a non-previewable Cell, rendering only once the pointer moves over a previewable Cell", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(3, 3, []) },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
    ]);
    expect(out[1].preview).toBeNull();
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
  });

  it("arms on a Revealed numeric Cell with no hidden neighbors, rendering once the pointer moves onto a previewable Cell", () => {
    // A Revealed numeric Cell whose neighbors are all Revealed or Flagged has
    // no Preview scope, but is still a valid Chord target (isNumericCell).
    const out = run([
      rightDown(previewable(1, 1, [], true)),
      { kind: "left-down", cell: previewable(1, 1, [], true) },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
      { kind: "left-up" },
    ]);
    expect(out[1].preview).toBeNull();
    expect(out[2].preview).toEqual({ pos: pos(2, 2), cells: [pos(1, 2)] });
    expect(out[3].action).toEqual({ type: "chord", row: 2, col: 2 });
  });

  it("reports 'flag' and 'reveal' for plain single-button presses", () => {
    const [flagOut] = run([rightDown(previewable(1, 2, [pos(0, 1)]))]);
    expect(flagOut.transition).toBe("flag");
    const [revealOut] = run([{ kind: "left-down", cell: previewable(3, 4, []) }]);
    expect(revealOut.transition).toBe("reveal");
  });

  it("reports 'armed' on arming, in either press order, even when the press was off a Cell", () => {
    const rightFirst = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
    ]);
    expect(rightFirst[1].transition).toBe("armed");
    const leftFirst = run([
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      rightDown(previewable(1, 1, [pos(0, 0)])),
    ]);
    expect(leftFirst[1].transition).toBe("armed");
    const offCell = run([
      { kind: "right-down", cell: null },
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
    ]);
    expect(offCell[1].transition).toBe("armed");
  });

  it("reports 'preview-set' when the Preview appears and 'preview-moved' when it follows the pointer", () => {
    // Arming over a non-previewable Cell leaves no Preview; it appears on
    // the first pointer-move over a previewable Cell, then moves with it.
    const out = run([
      rightDown(previewable(1, 1, [])),
      { kind: "left-down", cell: previewable(1, 1, []) },
      { kind: "pointer-move", cell: previewable(2, 2, [pos(1, 2)]) },
      { kind: "pointer-move", cell: previewable(3, 3, [pos(2, 3)]) },
    ]);
    expect(out[1].transition).toBe("armed");
    expect(out[2].transition).toBe("preview-set");
    expect(out[3].transition).toBe("preview-moved");
  });

  it("reports 'preview-cleared' when the Preview disappears, and nothing when it was already absent", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-move", cell: previewable(3, 3, []) },
      { kind: "pointer-move", cell: previewable(4, 4, []) },
    ]);
    expect(out[2].transition).toBe("preview-cleared");
    expect(out[3].transition).toBeUndefined();
  });

  it("reports 'chord' when the Chord solves and 'disarmed' on a plain release", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "left-up" },
    ]);
    expect(out[2].transition).toBe("chord");
    const [plainUp] = run([{ kind: "left-up" }]);
    expect(plainUp.transition).toBe("disarmed");
  });

  it("reports 'preview-cleared' on pointer-leave with a Preview shown and 'disarmed' on blur", () => {
    const out = run([
      rightDown(previewable(1, 1, [pos(0, 0)])),
      { kind: "left-down", cell: previewable(1, 1, [pos(0, 0)]) },
      { kind: "pointer-leave" },
      { kind: "blur" },
    ]);
    expect(out[2].transition).toBe("preview-cleared");
    expect(out[3].transition).toBe("disarmed");
  });
});
