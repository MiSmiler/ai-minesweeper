import type { Action, GameSnapshot } from "./api";
import { log } from "./log";

/** The deps a page-nav reset needs: how to start a new game and how to
 * navigate away (injected so the handler is testable without a real
 * location). */
export interface PageNavDeps {
  post: (action: Action) => Promise<GameSnapshot>;
  navigate: (url: string) => void;
}

/** Navigates the window to `url`. The real navigation shim, injected into the
 * page-nav wiring; tests substitute a spy. */
export function navigateTo(url: string): void {
  window.location.href = url;
}

/** Returns a click handler that starts a fresh Game, then navigates — used by
 * the cross-page jump links so a jump always lands on a new Board. The new
 * game is fired first so the destination page renders a fresh position. */
export function createNavReset(
  deps: PageNavDeps,
  href: string,
): (ev: Event) => void {
  return (ev) => {
    ev.preventDefault();
    // The new game always fires before the jump; if it fails we still
    // navigate (the destination renders the current position) but log it.
    void deps
      .post({ type: "new-game" })
      .catch((err) => {
        const message = err instanceof Error ? err.message : err;
        log.error(`Page nav: new-game failed: ${message}`);
      })
      .finally(() => deps.navigate(href));
  };
}

/** Wires every `[data-nav]` link in `container` with a reset-then-navigate
 * handler, so jumping between the human and ai pages always starts a new
 * Game. */
export function wirePageNav(container: HTMLElement, deps: PageNavDeps): void {
  container
    .querySelectorAll<HTMLAnchorElement>("[data-nav]")
    .forEach((link) => {
      const href = link.getAttribute("href") ?? "/";
      link.addEventListener("click", createNavReset(deps, href));
    });
}
