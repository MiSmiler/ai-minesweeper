import { describe, expect, it, vi } from "vitest";
import type { Action, GameSnapshot } from "../api";
import { createActionController } from "./controller";
import { gameState } from "../../infra/testUtils";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const reveal: Action = { type: "reveal", row: 0, col: 0 };
const flag: Action = { type: "flag", row: 1, col: 1 };

describe("createActionController", () => {
  it("applies an action through post and resolves with the fresh state", async () => {
    const post = vi.fn().mockResolvedValue(gameState({ elapsed_secs: 5 }));
    const controller = createActionController(post);
    await expect(controller.apply(reveal)).resolves.toEqual(
      gameState({ elapsed_secs: 5 }),
    );
    expect(post).toHaveBeenCalledWith(reveal);
  });

  it("resolves the state of every non-superseded action in order", async () => {
    const post = vi.fn().mockResolvedValue(gameState({ elapsed_secs: 7 }));
    const controller = createActionController(post);
    await expect(controller.apply(reveal)).resolves.toEqual(
      gameState({ elapsed_secs: 7 }),
    );
    await expect(controller.apply(flag)).resolves.toEqual(
      gameState({ elapsed_secs: 7 }),
    );
  });

  it("drops the stale response of a superseded action", async () => {
    const first = deferred<GameSnapshot>();
    const second = deferred<GameSnapshot>();
    let call = 0;
    const post = vi.fn(() => (call++ === 0 ? first.promise : second.promise));
    const controller = createActionController(post);

    const pendingFirst = controller.apply(reveal);
    const pendingSecond = controller.apply(flag);
    // The newer action resolves first; the stale one resolves later.
    second.resolve(gameState({ elapsed_secs: 2 }));
    first.resolve(gameState({ elapsed_secs: 1 }));

    await expect(pendingFirst).resolves.toBeNull();
    await expect(pendingSecond).resolves.toEqual(
      gameState({ elapsed_secs: 2 }),
    );
  });
});
