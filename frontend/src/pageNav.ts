import type { Action, GameSnapshot } from "./api";
import { log } from "./log";

/** The backend adapters the page navigation starts a fresh Game through —
 * the same the game page uses, so both pages act on the one shared backend
 * Game. */
export interface PageNavDeps {
  /** Reads the current Game snapshot (to learn the active Difficulty). */
  fetchState: () => Promise<GameSnapshot>;
  /** Sends an action; the new-game the navigation starts. */
  post: (action: Action) => Promise<GameSnapshot>;
}

/** Starts a fresh Game — keeping the current Difficulty — then navigates to
 * `target`. Both pages share one backend Game, so the leaving page starts
 * the new Game before the target page loads and renders it.
 *
 * Best-effort: a failure to start the Game is logged and the navigation
 * still proceeds (the target page shows whatever Game the backend holds). */
export async function startNewGameAndNavigate(
  target: string,
  deps: PageNavDeps,
  navigate: (target: string) => void,
): Promise<void> {
  try {
    const state = await deps.fetchState();
    await deps.post({ type: "new-game", difficulty: state.difficulty });
  } catch (err) {
    const message = err instanceof Error ? err.message : err;
    log.error(`Failed to start a new game before navigating: ${message}`);
  }
  navigate(target);
}

/** Wires every `<a data-nav>` link in `container` so clicking it starts a
 * fresh Game (keeping the current Difficulty) before navigating to the
 * link's `href`. `navigate` is injectable for tests (defaults to a
 * `window.location` assignment). */
export function wirePageNav(
  container: ParentNode,
  deps: PageNavDeps,
  navigate: (target: string) => void = (target) => {
    window.location.href = target;
  },
): void {
  container
    .querySelectorAll<HTMLAnchorElement>("a[data-nav]")
    .forEach((link) => {
      link.addEventListener("click", (event) => {
        event.preventDefault();
        const target = link.getAttribute("href");
        if (!target) return;
        void startNewGameAndNavigate(target, deps, navigate);
      });
    });
}
