import { describe, expect, it } from "vitest";
import type { Pos } from "./api";
import {
  createGestureMachine,
  type CellHit,
  type GestureEvent,
  type GestureOutput,
} from "./gesture";

const pos = (row: number, col: number): Pos => ({ row, col });

/** Runs a sequence of gesture events through a fresh machine, returning the
 * output of every event in order. */
function run(events: GestureEvent[]): GestureOutput[] {
  const machine = createGestureMachine();
  return events.map((event) => machine.handle(event));
}

/** A hit-tested Cell for the machine. Defaults: a previewable Cell is a
 * Revealed numeric Cell; pass `isNumericCell`/`isRevealed` explicitly for
 * the in-between cases (Revealed but not numeric, hidden, off-board). */
const previewable = (
  row: number,
  col: number,
  previewCells: Pos[],
  isNumericCell = previewCells.length > 0,
  isRevealed = isNumericCell,
): CellHit => ({
  pos: pos(row, col),
  previewCells,
  isNumericCell,
  isRevealed,
});

const rightDown = (cell: CellHit | null): GestureEvent => ({
  kind: "right-down",
  cell,
});
const leftDown = (cell: CellHit | null): GestureEvent => ({
  kind: "left-down",
  cell,
});
const pointerMove = (cell: CellHit | null): GestureEvent => ({
  kind: "pointer-move",
  cell,
});

describe("createGestureMachine", () => {
  describe("flag and idle", () => {
    it("sends a Flag on right-down on a Hidden Cell, reporting it as an action", () => {
      const [out] = run([rightDown(previewable(1, 2, [], false, false))]);
      expect(out.action).toEqual({ type: "flag", row: 1, col: 2 });
      expect(out.effects).toEqual(["flag"]);
      expect(out.phaseChange).toBeUndefined();
      expect(out.pressPreview).toBeNull();
      expect(out.chordPreview).toBeNull();
    });

    it("does not send a Flag on right-down on a Revealed Cell", () => {
      const [out] = run([rightDown(previewable(1, 2, [pos(0, 1)]))]);
      expect(out.action).toBeUndefined();
      expect(out.effects).toEqual([]);
      expect(out.phaseChange).toBeUndefined();
      expect(out.pressPreview).toBeNull();
      expect(out.chordPreview).toBeNull();
    });

    it("does nothing on right-down off a Cell", () => {
      const [out] = run([rightDown(null)]);
      expect(out.action).toBeUndefined();
      expect(out.effects).toEqual([]);
      expect(out.phaseChange).toBeUndefined();
    });

    it("does nothing on left-down off a Cell", () => {
      const [out] = run([leftDown(null)]);
      expect(out.action).toBeUndefined();
      expect(out.effects).toEqual([]);
      expect(out.phaseChange).toBeUndefined();
    });

    it("does nothing on pointer-move, left-up, right-up or blur while idle", () => {
      const out = run([
        pointerMove(previewable(1, 1, [pos(0, 0)])),
        { kind: "left-up" },
        { kind: "right-up" },
        { kind: "blur" },
      ]);
      for (const o of out) {
        expect(o.action).toBeUndefined();
        expect(o.effects).toEqual([]);
        expect(o.phaseChange).toBeUndefined();
      }
    });
  });

  describe("press and reveal (#38)", () => {
    it("presses: left-down shows the Press Preview and defers the Reveal", () => {
      const [out] = run([leftDown(previewable(3, 4, []))]);
      expect(out.action).toBeUndefined();
      expect(out.phaseChange).toBe("pressed");
      expect(out.effects).toEqual(["press-set"]);
      expect(out.pressPreview).toEqual(pos(3, 4));
    });

    it("reveals on left-up the Cell under the pointer", () => {
      const out = run([
        leftDown(previewable(1, 1, [])),
        pointerMove(previewable(2, 2, [])),
        { kind: "left-up" },
      ]);
      expect(out[2].action).toEqual({ type: "reveal", row: 2, col: 2 });
      expect(out[2].phaseChange).toBe("released");
      expect(out[2].effects).toEqual(["reveal"]);
    });

    it("moves the Press Preview on pointer-move over another Cell", () => {
      const out = run([
        leftDown(previewable(1, 1, [])),
        pointerMove(previewable(2, 2, [])),
      ]);
      expect(out[1].pressPreview).toEqual(pos(2, 2));
      expect(out[1].effects).toEqual(["press-moved"]);
    });

    it("ignores pointer-move over the same Cell while pressing", () => {
      const out = run([
        leftDown(previewable(1, 1, [])),
        pointerMove(previewable(1, 1, [])),
      ]);
      expect(out[1].effects).toEqual([]);
      expect(out[1].pressPreview).toEqual(pos(1, 1));
    });

    it("clears the Press Preview on pointer-move off a Cell, releasing then does nothing", () => {
      const out = run([
        leftDown(previewable(1, 1, [])),
        pointerMove(null),
        { kind: "left-up" },
      ]);
      expect(out[1].effects).toEqual(["press-cleared"]);
      expect(out[1].pressPreview).toBeNull();
      expect(out[2].action).toBeUndefined();
      expect(out[2].phaseChange).toBe("released");
      expect(out[2].effects).toEqual([]);
    });

    it("cancels the press on pointer-leave, dropping its Arm eligibility", () => {
      const out = run([
        leftDown(previewable(1, 1, [pos(0, 0)])),
        { kind: "pointer-leave" },
        rightDown(previewable(2, 2, [pos(1, 2)])),
        { kind: "left-up" },
      ]);
      expect(out[1].effects).toEqual(["press-cleared"]);
      // The cancelled press cannot arm a later Chord…
      expect(out[2].phaseChange).toBeUndefined();
      expect(out[2].action).toBeUndefined();
      // …and its release reveals nothing.
      expect(out[3].action).toBeUndefined();
    });
  });

  describe("arming (right first)", () => {
    it("arms on left-down while Right is held on a Revealed Cell, showing the Chord Preview immediately", () => {
      const cells = [pos(0, 0), pos(0, 1)];
      const out = run([
        rightDown(previewable(1, 1, cells)),
        leftDown(previewable(1, 1, cells)),
      ]);
      expect(out[1].action).toBeUndefined();
      expect(out[1].phaseChange).toBe("armed");
      expect(out[1].effects).toEqual(["preview-set"]);
      expect(out[1].chordPreview).toEqual({ pos: pos(1, 1), cells });
      expect(out[1].pressPreview).toBeNull();
    });

    it("arms over a Revealed non-numeric Cell with no Preview, rendering on the first move", () => {
      const out = run([
        rightDown(previewable(1, 1, [], false, true)),
        leftDown(previewable(1, 1, [], false, true)),
        pointerMove(previewable(2, 2, [pos(1, 2)])),
        { kind: "left-up" },
      ]);
      expect(out[1].phaseChange).toBe("armed");
      expect(out[1].effects).toEqual([]);
      expect(out[1].chordPreview).toBeNull();
      expect(out[2].effects).toEqual(["preview-set"]);
      expect(out[3].action).toEqual({ type: "chord", row: 2, col: 2 });
    });

    it("does not arm when Right was pressed off a Cell", () => {
      const out = run([
        rightDown(null),
        leftDown(previewable(1, 1, [pos(0, 0)])),
      ]);
      expect(out[1].phaseChange).toBe("pressed");
      expect(out[1].effects).toEqual(["press-set"]);
      expect(out[1].action).toBeUndefined();
    });

    it("does not arm when the Right press landed on a non-Revealed Cell", () => {
      const out = run([
        rightDown(previewable(1, 1, [], false, false)),
        leftDown(previewable(2, 2, [pos(1, 2)], true, true)),
      ]);
      expect(out[1].phaseChange).toBe("pressed");
      expect(out[1].action).toBeUndefined();
    });

    it("does not arm when the Left press lands on a non-Revealed Cell", () => {
      const out = run([
        rightDown(previewable(1, 1, [pos(0, 0)])),
        leftDown(previewable(2, 2, [], false, false)),
        { kind: "left-up" },
      ]);
      expect(out[1].phaseChange).toBe("pressed");
      expect(out[1].pressPreview).toEqual(pos(2, 2));
      expect(out[2].action).toEqual({ type: "reveal", row: 2, col: 2 });
    });
  });

  describe("arming (left first)", () => {
    it("arms on right-down while Left is held on a Revealed Cell, showing the Chord Preview immediately and sending no Flag", () => {
      const cells = [pos(0, 0)];
      const out = run([
        leftDown(previewable(1, 1, cells)),
        rightDown(previewable(1, 1, cells)),
      ]);
      expect(out[0].action).toBeUndefined();
      expect(out[0].phaseChange).toBe("pressed");
      expect(out[1].phaseChange).toBe("armed");
      expect(out[1].effects).toEqual(["preview-set"]);
      expect(out[1].chordPreview).toEqual({ pos: pos(1, 1), cells });
      expect(out[1].action).toBeUndefined();
    });

    it("does not arm when the Right press lands on a non-Revealed Cell", () => {
      const out = run([
        leftDown(previewable(1, 1, [pos(0, 0)])),
        rightDown(previewable(2, 2, [], false, false)),
        { kind: "left-up" },
      ]);
      expect(out[1].phaseChange).toBeUndefined();
      expect(out[1].effects).toEqual([]);
      expect(out[1].action).toBeUndefined();
      expect(out[2].action).toEqual({ type: "reveal", row: 1, col: 1 });
    });
  });

  describe("armed behavior", () => {
    const armed = (): GestureEvent[] => [
      rightDown(previewable(1, 1, [pos(0, 0)])),
      leftDown(previewable(1, 1, [pos(0, 0)])),
    ];

    it("sends a Chord on left-up while armed and clears the Preview", () => {
      const out = run([...armed(), { kind: "left-up" }]);
      expect(out[2].action).toEqual({ type: "chord", row: 1, col: 1 });
      expect(out[2].phaseChange).toBe("disarmed");
      expect(out[2].effects).toEqual(["chord", "preview-cleared"]);
      expect(out[2].chordPreview).toBeNull();
    });

    it("sends nothing on left-up while armed with no Preview shown", () => {
      const out = run([
        rightDown(previewable(1, 1, [], false, true)),
        leftDown(previewable(1, 1, [], false, true)),
        { kind: "left-up" },
      ]);
      expect(out[2].action).toBeUndefined();
      expect(out[2].phaseChange).toBe("disarmed");
      expect(out[2].effects).toEqual([]);
    });

    it("keeps the Preview armed on right-up, so a later left-up still sends a Chord", () => {
      const out = run([...armed(), { kind: "right-up" }, { kind: "left-up" }]);
      expect(out[2].effects).toEqual([]);
      expect(out[3].action).toEqual({ type: "chord", row: 1, col: 1 });
    });

    it("drives the Preview by Left alone after right-up", () => {
      const out = run([
        ...armed(),
        { kind: "right-up" },
        pointerMove(previewable(2, 2, [pos(1, 2)])),
      ]);
      expect(out[3].effects).toEqual(["preview-moved"]);
      expect(out[3].chordPreview).toEqual({
        pos: pos(2, 2),
        cells: [pos(1, 2)],
      });
    });

    it("re-arms on a fresh Left press while Right is still held after a Chord", () => {
      const out = run([
        ...armed(),
        { kind: "left-up" },
        leftDown(previewable(2, 2, [pos(1, 2)])),
      ]);
      expect(out[3].phaseChange).toBe("armed");
      expect(out[3].chordPreview).toEqual({
        pos: pos(2, 2),
        cells: [pos(1, 2)],
      });
    });

    it("moves the Preview on pointer-move over a new previewable Cell while armed", () => {
      const out = run([
        ...armed(),
        pointerMove(previewable(2, 2, [pos(1, 2)])),
      ]);
      expect(out[2].effects).toEqual(["preview-moved"]);
      expect(out[2].chordPreview).toEqual({
        pos: pos(2, 2),
        cells: [pos(1, 2)],
      });
    });

    it("clears the Preview on pointer-move over a non-numeric Cell while armed", () => {
      const out = run([
        ...armed(),
        pointerMove(previewable(3, 3, [], false, true)),
      ]);
      expect(out[2].effects).toEqual(["preview-cleared"]);
      expect(out[2].chordPreview).toBeNull();
    });

    it("clears the Preview on pointer-move off a Cell while armed", () => {
      const out = run([...armed(), pointerMove(null)]);
      expect(out[2].effects).toEqual(["preview-cleared"]);
    });

    it("re-renders the Preview on pointer-move back over a previewable Cell after leaving", () => {
      const out = run([
        ...armed(),
        pointerMove(null),
        pointerMove(previewable(2, 2, [pos(1, 2)])),
      ]);
      expect(out[2].effects).toEqual(["preview-cleared"]);
      expect(out[3].effects).toEqual(["preview-set"]);
    });

    it("sends no Chord on left-up when the Preview was cleared by moving off", () => {
      const out = run([...armed(), pointerMove(null), { kind: "left-up" }]);
      expect(out[3].action).toBeUndefined();
      expect(out[3].phaseChange).toBe("disarmed");
      expect(out[3].effects).toEqual([]);
    });

    it("clears the Preview on pointer-leave but keeps the gesture armed", () => {
      const out = run([
        ...armed(),
        { kind: "pointer-leave" },
        pointerMove(previewable(1, 1, [pos(0, 0)])),
        { kind: "left-up" },
      ]);
      expect(out[2].effects).toEqual(["preview-cleared"]);
      expect(out[3].effects).toEqual(["preview-set"]);
      expect(out[4].action).toEqual({ type: "chord", row: 1, col: 1 });
    });
  });

  describe("blur", () => {
    it("disarms on blur, clearing the Preview and forgetting the buttons", () => {
      const out = run([
        rightDown(previewable(1, 1, [pos(0, 0)])),
        leftDown(previewable(1, 1, [pos(0, 0)])),
        { kind: "blur" },
        pointerMove(previewable(2, 2, [pos(1, 2)])),
        { kind: "left-up" },
      ]);
      expect(out[2].phaseChange).toBe("disarmed");
      expect(out[2].effects).toEqual(["preview-cleared"]);
      expect(out[3].effects).toEqual([]);
      expect(out[4].action).toBeUndefined();
    });

    it("clears the Press Preview on blur during a press", () => {
      const out = run([leftDown(previewable(1, 1, [])), { kind: "blur" }]);
      expect(out[1].phaseChange).toBe("released");
      expect(out[1].effects).toEqual(["press-cleared"]);
    });
  });
});
