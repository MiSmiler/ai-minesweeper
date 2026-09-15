// The `app/` composition layer seam (ADR-0012): how a PlayMode is named,
// mounted, and switched. The concept `PlayMode` (camel `HumanPlay`/`AiPlay`)
// and its runtime identifier `PlayModeName` (`human`/`ai`) are
// deliberately not unified: one is the UI-facing concept, the other is the
// runtime key.

import type { AiApi } from "../ai-player/api";
import { composeAiPlayMode } from "./aiPlayMode";
import { composeHumanPlayMode } from "./humanPlayMode";

/** The runtime identifier of a PlayMode. */
export type PlayModeName = "human" | "ai";

/** Screenshots the board into a data URL — the `ai-player/screenshot.ts` signature.
 * Injected (rather than imported) so jsdom tests can substitute it, since the
 * browser-only capture never runs under jsdom. `createBoardAxis` is pure DOM
 * and is imported directly by the compositions, not injected. */
export type CaptureBoardImage = (
  boardEl: HTMLElement,
  opts?: { pixelRatio?: number },
) => Promise<string>;

/** The dependencies a mode composition receives (injected by `main.ts`). */
export interface AppDeps {
  /** Reads the initial PlayMode the app should start in. */
  getPlayMode(): PlayModeName;
  /** The ai-player slice entry point (a stub/mock in this ticket). */
  aiApi: AiApi;
  /** Screenshots the board for the image input form. */
  captureBoardImage: CaptureBoardImage;
}

/** A mounted PlayMode composition, with an optional guard the shell consults
 * before discarding it (mode switch) or on a page unload (refresh). Only the
 * AiPlay composition implements the guard — it is the only one that holds a
 * live AI Session whose loss a refresh / switch would silently discard
 * (issue #112 US-32: any clearing operation asks first). */
export interface Composition {
  /** Tears down the composition; the current Game is abandoned (ADR-0012). */
  dispose(): void;
  /** True while the AI Session holds at least one committed Turn — the
   * `non-empty` half of the session's `empty` / `non-empty` predicate
   * (ADR-0017), the same predicate that locks the InputMode select and gates
   * every discard confirm. */
  hasNonEmptySession?(): boolean;
  /** Blocking confirm before discarding the non-empty AI Session; returns true
   * to proceed. `message` is context-specific. Absent when there is nothing
   * to discard — callers treat `undefined` as "proceed". */
  confirmDiscard?(message: string): boolean;
}

/** Mounts the composition for a mode into `root`. Switching modes = dispose
 * the current composition and mount a new one — the current Game is abandoned
 * and a fresh one starts (ADR-0012). Returns the composition so the shell can
 * guard a session-bearing switch. */
export function mountMode(
  mode: PlayModeName,
  root: HTMLElement,
  deps: AppDeps,
): Composition {
  return mode === "ai"
    ? composeAiPlayMode(root, deps)
    : composeHumanPlayMode(root, deps);
}

const MODES: ReadonlyArray<[PlayModeName, string]> = [
  ["human", "Human"],
  ["ai", "AI"],
];

/** Renders the top-bar mode switcher (Human / AI) into `root`,
 * highlighting `current`, and calls `onSwitch` with the clicked mode. */
export function renderModeSwitcher(
  root: HTMLElement,
  current: PlayModeName,
  onSwitch: (mode: PlayModeName) => void,
): void {
  const bar = document.createElement("div");
  bar.className = "mode-switcher";
  for (const [mode, label] of MODES) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `mode-btn${mode === current ? " active" : ""}`;
    btn.dataset.mode = mode;
    btn.textContent = label;
    btn.addEventListener("click", () => onSwitch(mode));
    bar.appendChild(btn);
  }
  root.replaceChildren(bar);
}
