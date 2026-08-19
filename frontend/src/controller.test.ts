import { describe, expect, it, vi } from "vitest";
import type { Action, CellView, GameState } from "./api";
import { createActionController } from "./controller";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function gameState(elapsed: number): GameState {
  const cells: CellView[] = [];
  for (let i = 0; i < 4; i++) {
    cells.push({ state: "hidden", content: null });
  }
  return {
    game_state: "playing",
    difficulty: "beginner",
    rows: 2,
    cols: 2,
    flags_remaining: 1,
    elapsed_secs: elapsed,
    trigger: null,
    cells,
  };
}

const reveal: Action = { type: "reveal", row: 0, col: 0 };
const flag: Action = { type: "flag", row: 1, col: 1 };

describe("createActionController", () => {
  it("applies an action through post and resolves with the fresh state", async () => {
    const post = vi.fn().mockResolvedValue(gameState(5));
    const controller = createActionController(post);
    await expect(controller.apply(reveal)).resolves.toEqual(gameState(5));
    expect(post).toHaveBeenCalledWith(reveal);
  });

  it("resolves the state of every non-superseded action in order", async () => {
    const post = vi.fn().mockResolvedValue(gameState(7));
    const controller = createActionController(post);
    await expect(controller.apply(reveal)).resolves.toEqual(gameState(7));
    await expect(controller.apply(flag)).resolves.toEqual(gameState(7));
  });

  it("drops the stale response of a superseded action", async () => {
    const first = deferred<GameState>();
    const second = deferred<GameState>();
    let call = 0;
    const post = vi.fn(() => (call++ === 0 ? first.promise : second.promise));
    const controller = createActionController(post);

    const pendingFirst = controller.apply(reveal);
    const pendingSecond = controller.apply(flag);
    // The newer action resolves first; the stale one resolves later.
    second.resolve(gameState(2));
    first.resolve(gameState(1));

    await expect(pendingFirst).resolves.toBeNull();
    await expect(pendingSecond).resolves.toEqual(gameState(2));
  });
});
