// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  startNewGameAndNavigate,
  wirePageNav,
  type PageNavDeps,
} from "./pageNav";
import { gameState } from "./testUtils";

function makeDeps(over: Partial<PageNavDeps> = {}): {
  deps: PageNavDeps;
  fetchState: ReturnType<typeof vi.fn>;
  post: ReturnType<typeof vi.fn>;
} {
  const fetchState = vi.fn(() =>
    Promise.resolve(gameState({ difficulty: "expert" })),
  );
  const post = vi.fn(() => Promise.resolve(gameState()));
  return { deps: { fetchState, post, ...over }, fetchState, post };
}

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("startNewGameAndNavigate", () => {
  it("reads the current Difficulty and starts a fresh Game of it, then navigates", async () => {
    const { deps, fetchState, post } = makeDeps();
    const navigate = vi.fn();

    await startNewGameAndNavigate("/ai.html", deps, navigate);

    expect(fetchState).toHaveBeenCalled();
    expect(post).toHaveBeenCalledWith({
      type: "new-game",
      difficulty: "expert",
    });
    expect(navigate).toHaveBeenCalledWith("/ai.html");
  });

  it("still navigates (and logs) when starting the new Game fails", async () => {
    const { deps, post } = makeDeps();
    post.mockRejectedValue(new Error("boom"));
    const navigate = vi.fn();

    await startNewGameAndNavigate("/ai.html", deps, navigate);

    expect(navigate).toHaveBeenCalledWith("/ai.html");
  });

  it("still navigates when the Difficulty cannot be read", async () => {
    const { deps, fetchState } = makeDeps();
    fetchState.mockRejectedValue(new Error("offline"));
    const navigate = vi.fn();

    await startNewGameAndNavigate("/ai.html", deps, navigate);

    expect(navigate).toHaveBeenCalledWith("/ai.html");
  });
});

describe("wirePageNav", () => {
  it("a data-nav link starts a new Game then navigates to its href", async () => {
    const { deps, post } = makeDeps();
    document.body.innerHTML = `
      <nav class="page-nav">
        <a data-nav href="/ai.html">AI</a>
      </nav>
    `;
    const container = document.querySelector("nav")!;
    const navigate = vi.fn();

    wirePageNav(container, deps, navigate);
    document
      .querySelector<HTMLAnchorElement>('a[data-nav][href="/ai.html"]')!
      .dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true }),
      );

    // Let the async new-game round trip settle.
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(post).toHaveBeenCalledWith({
      type: "new-game",
      difficulty: "expert",
    });
    expect(navigate).toHaveBeenCalledWith("/ai.html");
  });

  it("leaves plain links alone", async () => {
    const { deps, post } = makeDeps();
    document.body.innerHTML = `
      <nav class="page-nav">
        <a href="#docs">Docs</a>
        <a data-nav href="/ai.html">AI</a>
      </nav>
    `;
    const navigate = vi.fn();

    wirePageNav(document.querySelector("nav")!, deps, navigate);
    document
      .querySelector<HTMLAnchorElement>('a[href="#docs"]')!
      .dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true }),
      );

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(post).not.toHaveBeenCalled();
    expect(navigate).not.toHaveBeenCalled();
  });
});
