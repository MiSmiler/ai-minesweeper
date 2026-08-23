import { fetchState, postAction } from "./api";
import { startGamePage } from "./gamePage";
import type { TopBarEls } from "./render";
import "./style.css";

const boardEl = document.getElementById("board")!;
const topBarEls: TopBarEls = {
  counter: document.getElementById("counter")!,
  smiley: document.getElementById("smiley")!,
  timer: document.getElementById("timer")!,
  difficultyRow: document.querySelector(".difficulty-row")!,
};

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
