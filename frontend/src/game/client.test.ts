// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { Action, CellView, GameSnapshot, Position } from "./api";
import { createGameClient } from "./client";
import type { TopBarEls } from "./render/snapshotRender";
import { makeGameSnapshot } from "../infra/testUtils";

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
    <div class="game-top-bar">
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

const pos = (row: number, col: number): Position => ({ row, col });

/** A client over the jsdom document with the given post/fetchSnapshot mocks. */
function makeClient(
  over: {
    post?: Mock<(action: Action) => Promise<GameSnapshot>>;
    fetchSnapshot?: Mock<() => Promise<GameSnapshot>>;
  } = {},
) {
  const post: Mock<(action: Action) => Promise<GameSnapshot>> =
    over.post ?? vi.fn().mockResolvedValue(makeGameSnapshot());
  const fetchSnapshot: Mock<() => Promise<GameSnapshot>> =
    over.fetchSnapshot ?? vi.fn().mockResolvedValue(makeGameSnapshot());
  const client = createGameClient({
    boardEl: boardEl(),
    topBarEls: topBarEls(),
    post,
    fetchSnapshot,
  });
  return { client, post, fetchSnapshot };
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
      const d = deferred<GameSnapshot>();
      const post = vi.fn(() => d.promise);
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" });

      // The response is pending: the press highlight is retained.
      expect(cell(0, 0)!.classList.contains("cell-preview")).toBe(true);

      d.resolve(makeGameSnapshot());
      await flush();
      // The fresh board has no retained highlight.
      expect(cell(0, 0)!.classList.contains("cell-preview")).toBe(false);
    });
  });

  describe("enable-gate cancellation (game over)", () => {
    it("cancels a held press when a response ends the game mid-gesture", async () => {
      const d = deferred<GameSnapshot>();
      const post = vi.fn(() => d.promise);
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" }); // Reveal in flight
      client.handleInput({ kind: "left-down", pos: pos(1, 1) }); // a new press
      expect(smiley().textContent).toBe("😮");

      // The response ends the game while the press is held.
      d.resolve(makeGameSnapshot({ game_state: "lost" }));
      await flush();

      // The gesture is cancelled: no surprise on the Lost board, and the
      // release sends no stray action.
      expect(smiley().textContent).toBe("😭");
      client.handleInput({ kind: "left-up" });
      expect(post).toHaveBeenCalledTimes(1);
    });

    it("ignores Board input once the game has ended", async () => {
      const d = deferred<GameSnapshot>();
      const post = vi.fn(() => d.promise);
      const { client } = makeClient({ post });
      await client.init();

      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" });
      d.resolve(makeGameSnapshot({ game_state: "won" }));
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
      const fetchSnapshot = vi
        .fn()
        .mockResolvedValueOnce(makeGameSnapshot({ elapsed_secs: 0 }))
        .mockResolvedValueOnce(makeGameSnapshot({ elapsed_secs: 65 }));
      const { client } = makeClient({ fetchSnapshot });
      await client.init();
      expect(timer().textContent).toBe("000");

      await client.pollTimer();
      expect(timer().textContent).toBe("065");
      // The counter and Smiley come from action responses, not the poll.
      expect(counter().textContent).toBe("001");
      expect(smiley().textContent).toBe("🙂");
    });
  });

  /** The latest-action gate (was createActionController): its behavior is
   * covered here at the client level — a superseding action's response
   * renders, the superseded one's drops. */
  describe("stale responses", () => {
    it("renders only the latest action's response", async () => {
      const reveal = deferred<GameSnapshot>();
      const flag = deferred<GameSnapshot>();
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
        makeGameSnapshot({
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
        makeGameSnapshot({
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

    it("renders each non-superseded action's response in order", async () => {
      const first = deferred<GameSnapshot>();
      const second = deferred<GameSnapshot>();
      let call = 0;
      const post = vi.fn(() => (call++ === 0 ? first.promise : second.promise));
      const { client } = makeClient({ post });
      await client.init();

      // First action: Reveal (0,0), left in flight.
      client.handleInput({ kind: "left-down", pos: pos(0, 0) });
      client.handleInput({ kind: "left-up" });

      // Its response lands while it is still the latest, so it renders.
      first.resolve(
        makeGameSnapshot({
          cells: [
            view("revealed", 1),
            view("hidden"),
            view("hidden"),
            view("hidden"),
          ],
        }),
      );
      await flush();
      expect(cell(0, 0)!.classList.contains("cell-revealed")).toBe(true);

      // Second action: Flag (1,1) — off the Cell the Reveal just opened, so
      // it stays Hidden and still sends a Flag.
      client.handleInput({ kind: "right-down", pos: pos(1, 1) });

      // Its response lands next, still the latest, so it too renders.
      second.resolve(
        makeGameSnapshot({
          flags_remaining: 0,
          cells: [
            view("revealed", 1),
            view("hidden"),
            view("hidden"),
            view("flagged"),
          ],
        }),
      );
      await flush();
      // Both responses rendered in order: the Reveal survives, the Flag lands.
      expect(cell(0, 0)!.classList.contains("cell-revealed")).toBe(true);
      expect(cell(1, 1)!.textContent).toBe("🚩");
      expect(counter().textContent).toBe("000");
    });
  });
});
