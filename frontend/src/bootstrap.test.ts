// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { gamePageEls } from "./bootstrap";

/** Builds a game-page container with the markup both pages share. */
function gamePageMarkup(): HTMLElement {
  const root = document.createElement("div");
  root.innerHTML = `
    <div id="app">
      <div class="difficulty-row">
        <button class="difficulty" data-difficulty="beginner">Beginner</button>
      </div>
      <div class="game-frame">
        <div class="top-bar">
          <div id="counter" class="led"></div>
          <button id="smiley" class="smiley">New game</button>
          <div id="timer" class="led"></div>
        </div>
        <div id="board"></div>
      </div>
    </div>`;
  return root;
}

describe("gamePageEls", () => {
  it("resolves the board and top-bar elements from the container", () => {
    const root = gamePageMarkup();
    const els = gamePageEls(root);
    expect(els.boardEl.id).toBe("board");
    expect(els.topBarEls.counter.id).toBe("counter");
    expect(els.topBarEls.smiley.id).toBe("smiley");
    expect(els.topBarEls.timer.id).toBe("timer");
    expect(els.topBarEls.difficultyRow.className).toBe("difficulty-row");
  });

  it("throws when a required element is missing", () => {
    const root = document.createElement("div");
    root.innerHTML = `<div id="app"><div class="difficulty-row"></div></div>`;
    expect(() => gamePageEls(root)).toThrow(/missing required elements/);
  });
});
