import { fetchState, postAction } from "./api";
import { queryGameFrame } from "./gameFrame";
import { startGamePage } from "./gamePage";
import { wirePageNav } from "./pageNav";
import "./style.css";

const { boardEl, topBarEls } = queryGameFrame();

// The ai page reuses the same game-page bootstrap (ADR-0007): the board is
// the same interactable Game client, in a page-specific frame (its own
// markup), with the AI panel placeholder alongside. The entry is a thin DOM
// lookup plus the swap-page navigation — the bootstrap owns the rest.
void startGamePage({
  boardEl,
  topBarEls,
  post: postAction,
  fetchState,
}).start();

// Either page swaps to the other by starting a fresh Game (keeping the
// current Difficulty) then navigating; both act on the one backend Game.
wirePageNav(document.getElementById("app")!, {
  fetchState,
  post: postAction,
});
