// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { Action, CellView, GameState, Pos } from "./api";
import { createGameClient } from "./client";
import type { TopBarEls } from "./render";
import { gameState } from "./testUtils";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Flushes the microtask queue plus one macrotask so pending action
 * responses settle. */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

const view = (
  state: CellView["state"],
  content: CellView["content"] = null,
): CellView => ({ state, content });

beforeEach(() => {
  document.body.innerHTML = `
    <div class="difficulty-row">
      <button data-difficulty="beginner">Beginner</button>
      <button data-difficulty="intermediate">Intermediate</button>
      <button data-difficulty="expert">Expert</button>
    </div>
    <div class="top-bar">
      <div id="counter" class="led"></div>
      <button id="smiley" class="smiley"></button>
      <div id="timer" class="led"></div>
    </div>
    <div id="board"></div>
  `;
});

const boardEl = (): HTMLElement => document.getElementById("board")!;
const topBarEls = (): TopBarEls => ({
  counter: document.getElementById("counter")!,
  smiley: document.getElementById("smiley")!,
  timer: document.getElementById("timer")!,
  difficultyRow: document.querySelector(".difficulty-row")!,
});
const smiley = (): HTMLElement => document.getElementById("smiley")!;
const counter = (): HTMLElement => document.getElementById("counter")!;
const timer = (): HTMLElement => document.getElementById("timer")!;
const cell = (row: number, col: number): HTMLElement | null =>
  document.querySelector(`.board [data-row="${row}"][data-col="${col}"]`);

const pos = (row: number, col: number): Pos => ({ row, col });

/** A client over the jsdom document with the given post/fetchState mocks. */
function makeClient(
  over: {
    post?: Mock<(action: Action) => Promise<GameState>>;
    fetchState?: Mock<() => Promise<GameState>>;
  } = {},
) {
  const post: Mock<(action: Action) => Promise<GameState>> =
    over.post ?? vi.fn().mockResolvedValue(gameState());
  const fetchState: Mock<() => Promise<GameState>> =
    over.fetchState ?? vi.fn().mockResolvedValue(gameState());
  const client = createGameClient({
    boardEl: boardEl(),
    topBarEls: topBarEls(),
    post,
    fetchState,
  });
  return { client, post, fetchState };
}

describe("createGameClient", () => {
  it("renders the initial state on init", async () => {
    const { client } = makeClient();
    await client.init();
    expect(cell(0, 0)).not.toBeNull();
    expect(counter().textContent).toBe("001");
    expect(smiley().textContent).toBe("🙂");
    expect(timer().textContent).toBe("000");
  });

  describe("preview highlight round trip", () => {
    it("keeps the Reveal highlight until the response re-renders", async () => {
      const d = deferred<GameState>();
      const post = vi.fn(() => d.promise);
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" });

      // The response is pending: the press highlight is retained.
      expect(cell(0, 0)!.classList.contains("cell-preview")).toBe(true);

      d.resolve(gameState());
      await flush();
      // The fresh board has no retained highlight.
      expect(cell(0, 0)!.classList.contains("cell-preview")).toBe(false);
    });
  });

  describe("enable-gate cancellation (game over)", () => {
    it("cancels a held press when a response ends the game mid-gesture", async () => {
      const d = deferred<GameState>();
      const post = vi.fn(() => d.promise);
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" }); // Reveal in flight
      client.handleInput({ kind: "left-down", pos: pos(1, 1) }); // a new press
      expect(smiley().textContent).toBe("😮");

      // The response ends the game while the press is held.
      d.resolve(gameState({ game_state: "lost" }));
      await flush();

      // The gesture is cancelled: no surprise on the Lost board, and the
      // release sends no stray action.
      expect(smiley().textContent).toBe("😭");
      client.handleInput({ kind: "left-up" });
      expect(post).toHaveBeenCalledTimes(1);
    });

    it("ignores Board input once the game has ended", async () => {
      const d = deferred<GameState>();
      const post = vi.fn(() => d.promise);
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" });
      d.resolve(gameState({ game_state: "won" }));
      await flush();

      // The ended Board is inert: a press produces no preview, no surprise,
      // and no action.
      client.handleInput({ kind: "left-down", pos: pos(1, 1) });
      expect(smiley().textContent).toBe("😎");
      expect(cell(1, 1)!.classList.contains("cell-preview")).toBe(false);
      client.handleInput({ kind: "left-up" });
      expect(post).toHaveBeenCalledTimes(1);
    });
  });

  describe("Smiley faces", () => {
    it("surprises while a press is held and restores on release", async () => {
      const { client } = makeClient();
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      expect(smiley().textContent).toBe("😮");
      client.handleInput({ kind: "left-up" });
      expect(smiley().textContent).toBe("🙂");
      await flush();
    });
  });

  describe("new game", () => {
    it("sends the difficulty for a switch and none for the Smiley Button", async () => {
      const { client, post } = makeClient();
      await client.init();

      client.newGame("expert");
      expect(post).toHaveBeenLastCalledWith({
        type: "new-game",
        difficulty: "expert",
      });
      await flush();

      client.newGame();
      expect(post).toHaveBeenLastCalledWith({ type: "new-game" });
      await flush();
    });
  });

  describe("timer poll", () => {
    it("updates only the Timer from the poll", async () => {
      const fetchState = vi
        .fn()
        .mockResolvedValueOnce(gameState({ elapsed_secs: 0 }))
        .mockResolvedValueOnce(gameState({ elapsed_secs: 65 }));
      const { client } = makeClient({ fetchState });
      await client.init();
      expect(timer().textContent).toBe("000");

      await client.pollTimer();
      expect(timer().textContent).toBe("065");
      // The counter and Smiley come from action responses, not the poll.
      expect(counter().textContent).toBe("001");
      expect(smiley().textContent).toBe("🙂");
    });
  });

  describe("stale responses", () => {
    it("renders only the latest action's response", async () => {
      const reveal = deferred<GameState>();
      const flag = deferred<GameState>();
      let call = 0;
      const post = vi.fn(() => (call++ === 0 ? reveal.promise : flag.promise));
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(1, 1) });
      client.handleInput({ kind: "left-up" }); // Reveal (first) in flight
      client.handleInput({ kind: "right-down", pos: pos(0, 0) }); // Flag (latest) in flight

      // The older action's response resolves first but is stale: the Flag
      // was sent later, so the Reveal never renders.
      reveal.resolve(
        gameState({
          cells: [
            view("hidden"),
            view("hidden"),
            view("hidden"),
            view("revealed", 1),
          ],
        }),
      );
      await flush();
      expect(cell(1, 1)!.classList.contains("cell-revealed")).toBe(false);

      flag.resolve(
        gameState({
          cells: [
            view("flagged"),
            view("hidden"),
            view("hidden"),
            view("hidden"),
          ],
        }),
      );
      await flush();
      expect(cell(0, 0)!.textContent).toBe("🚩");
      expect(cell(1, 1)!.classList.contains("cell-revealed")).toBe(false);
    });
  });
});
