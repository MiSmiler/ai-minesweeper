import type { Action, GameState } from "./api";

/** Applies Actions to the server, dropping out-of-order responses: only the
 * latest action's result is kept, so a slow earlier response can never show
 * stale state. */
export function createActionController(post: (action: Action) => Promise<GameState>) {
  let seq = 0;
  return {
    /** Resolves to the applied state, or null when a newer Action
     * superseded this one while it was in flight. */
    async apply(action: Action): Promise<GameState | null> {
      const id = ++seq;
      const next = await post(action);
      return id === seq ? next : null;
    },
  };
}
