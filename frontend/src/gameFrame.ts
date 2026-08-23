import type { TopBarEls } from "./render";

/** The game-frame elements a page hands to the game-page bootstrap. */
export interface GameFrameEls {
  boardEl: HTMLElement;
  topBarEls: TopBarEls;
}

/** Looks up the reusable game frame's elements from the document. Both pages
 * (index.html and ai.html) share the same ids/selectors for the game frame,
 * so the entry points own this one lookup rather than each re-specifying the
 * DOM ids — a page that reuses the bootstrap needs only its own markup. */
export function queryGameFrame(): GameFrameEls {
  return {
    boardEl: document.getElementById("board")!,
    topBarEls: {
      counter: document.getElementById("counter")!,
      smiley: document.getElementById("smiley")!,
      timer: document.getElementById("timer")!,
      difficultyRow: document.querySelector(".difficulty-row")!,
    },
  };
}
