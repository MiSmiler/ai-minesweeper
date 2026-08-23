import { fetchState, postAction } from "./api";
import { queryGameFrame } from "./gameFrame";
import { startGamePage } from "./gamePage";
import { wirePageNav } from "./pageNav";
import "./style.css";

const { boardEl, topBarEls } = queryGameFrame();

// The entry point is a thin DOM lookup: the reusable game-page bootstrap owns
// the client module, the geometry/hit-testing, the input listeners, and the
// difficulty/Smiley delegation (ADR-0007). This file only hands the elements
// to it and starts it.
void startGamePage({
  boardEl,
  topBarEls,
  post: postAction,
  fetchState,
}).start();

// Swapping to the ai page starts a fresh Game (keeping the current
// Difficulty) then navigates; both pages act on the one backend Game.
wirePageNav(document.getElementById("app")!, {
  fetchState,
  post: postAction,
});
