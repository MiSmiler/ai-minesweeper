// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { createNavReset, wirePageNav } from "./nav";
import { gameState } from "./testUtils";

/** Flushes the microtask queue so a resolved new-game settles. */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

describe("createNavReset", () => {
  it("posts new-game then navigates to the link's href", async () => {
    const post = vi.fn().mockResolvedValue(gameState());
    const navigate = vi.fn();
    const handler = createNavReset({ post, navigate }, "/ai.html");
    const ev = new Event("click", { cancelable: true });

    handler(ev);

    expect(ev.defaultPrevented).toBe(true);
    expect(post).toHaveBeenCalledWith({ type: "new-game" });
    await flush();
    expect(navigate).toHaveBeenCalledWith("/ai.html");
  });

  it("navigates even when the new-game request fails", async () => {
    const post = vi.fn().mockRejectedValue(new Error("boom"));
    const navigate = vi.fn();

    createNavReset(
      { post, navigate },
      "/",
    )(new Event("click", { cancelable: true }));
    await flush();

    expect(navigate).toHaveBeenCalledWith("/");
  });
});

describe("wirePageNav", () => {
  it("wires every [data-nav] link", async () => {
    const root = document.createElement("div");
    root.innerHTML = `
      <a data-nav href="/ai.html">to ai</a>
      <a data-nav href="/">to human</a>
      <a href="/plain">plain</a>`;
    const post = vi.fn().mockResolvedValue(gameState());
    const navigate = vi.fn();

    wirePageNav(root, { post, navigate });

    const links = root.querySelectorAll<HTMLAnchorElement>("a");
    links[0].dispatchEvent(new Event("click", { cancelable: true }));
    links[1].dispatchEvent(new Event("click", { cancelable: true }));
    await flush();

    expect(post).toHaveBeenCalledTimes(2);
    expect(navigate).toHaveBeenCalledWith("/ai.html");
    expect(navigate).toHaveBeenCalledWith("/");
  });
});
