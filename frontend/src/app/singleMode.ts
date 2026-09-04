// The `SinglePlay` composition (the existing single-mode UI, extracted from
// the old `main.ts`). It is the game area alone — the player plays unaided.

import { createGameArea } from "./gameArea";
import type { AppDeps } from "./mode";

export interface Composition {
  dispose(): void;
}

/** Mounts the SinglePlay composition (an independent game area) into `root`.
 * `deps` is part of the seam but unused here — SinglePlay touches no AI. */
export function composeSingleMode(
  root: HTMLElement,
  deps: AppDeps,
): Composition {
  void deps;
  const area = createGameArea(root);
  return { dispose: area.dispose };
}
