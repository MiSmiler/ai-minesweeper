// @vitest-environment jsdom

// Tests for the SessionBox renderer (issue #119): reasoning is
// collapsible, content is plain, the coordinate stays text, and an interrupt
// renders a red tail line.

import { beforeEach, describe, expect, it } from "vitest";
import { createSessionBox } from "./sessionBox";

let container: HTMLElement;

beforeEach(() => {
  container = document.createElement("div");
  container.className = "session-stream";
  document.body.appendChild(container);
});

function render(
  state: Parameters<ReturnType<typeof createSessionBox>["render"]>[0],
): void {
  createSessionBox(container).render(state);
}

/** jsdom does no layout, so scroll metrics must be mocked; setting `scrollTop`
 * does not fire `scroll` in jsdom, so tests dispatch it manually to simulate
 * the user scrolling (issue #128). */
function mockScrollMetrics(
  el: HTMLElement,
  clientHeight: number,
  scrollHeight: number,
): void {
  Object.defineProperty(el, "clientHeight", {
    value: clientHeight,
    configurable: true,
  });
  Object.defineProperty(el, "scrollHeight", {
    value: scrollHeight,
    configurable: true,
  });
}

describe("createSessionBox", () => {
  it("renders reasoning and content into their blocks", () => {
    render({
      phase: "running",
      reasoning: "think",
      content: "(2,3)",
      user: "",
    });
    expect(container.querySelector(".session-reasoning")!.textContent).toBe(
      "think",
    );
    expect(container.querySelector(".session-content")!.textContent).toBe(
      "(2,3)",
    );
  });

  it("makes the reasoning block a collapsible details, content not", () => {
    render({ phase: "running", reasoning: "think", content: "hi", user: "" });
    // The reasoning block is inside a <details> collapsible.
    expect(container.querySelector(".session-collapse")).toBeTruthy();
    expect(
      container.querySelector(".session-reasoning")!.closest("details"),
    ).toBeTruthy();
    // Content is not wrapped in a collapsible.
    expect(
      container.querySelector(".session-content")!.closest("details"),
    ).toBeNull();
  });

  it("expands the reasoning block by default and keeps it open across renders", () => {
    const c = createSessionBox(container);
    const details = container.querySelector(
      "details.session-collapse",
    ) as HTMLDetailsElement;
    expect(details.open).toBe(true);

    // The user-visible `open` state is preserved when streaming updates arrive.
    c.render({ phase: "running", reasoning: "更多", content: "x", user: "" });
    expect(details.open).toBe(true);
    c.render({ phase: "done", reasoning: "结论", content: "y", user: "" });
    expect(details.open).toBe(true);
  });

  it("renders the caller's interrupt as a red tail line, not an alert", () => {
    render({
      phase: "interrupted",
      reasoning: "r",
      content: "c",
      user: "",
    });
    expect(container.querySelector(".session-interrupt")!.textContent).toBe(
      "已中断",
    );
  });

  it("keeps the coordinate as plain text (no parsing, no highlighting)", () => {
    render({
      phase: "done",
      reasoning: "",
      content: "(2,3)",
      user: "",
    });
    const content = container.querySelector(".session-content")!;
    expect(content.textContent).toBe("(2,3)");
    // Plain text: no child elements, no addresses parsed into markup.
    expect(content.querySelectorAll("*")).toHaveLength(0);
  });

  it("hides the empty reasoning/content/interrupt blocks", () => {
    render({ phase: "idle", reasoning: "", content: "", user: "" });
    const collapse = container.querySelector(
      ".session-collapse",
    ) as HTMLElement;
    const content = container.querySelector(".session-content") as HTMLElement;
    const interrupt = container.querySelector(
      ".session-interrupt",
    ) as HTMLElement;
    expect(collapse.style.display).toBe("none");
    expect(content.style.display).toBe("none");
    expect(interrupt.style.display).toBe("none");
  });

  it("shows interleaved reasoning and content as accumulated blocks", () => {
    const c = createSessionBox(container);
    c.render({ phase: "running", reasoning: "思考", content: "", user: "" });
    c.render({
      phase: "running",
      reasoning: "思考中",
      content: "答案出来",
      user: "",
    });
    expect(container.querySelector(".session-reasoning")!.textContent).toBe(
      "思考中",
    );
    expect(container.querySelector(".session-content")!.textContent).toBe(
      "答案出来",
    );
  });

  it("renders the player message boxed, before the AI reasoning block", () => {
    render({
      phase: "done",
      reasoning: "think",
      content: "(2,3)",
      user: "0F\n2.",
    });
    const user = container.querySelector(".session-user")!;
    const userText = container.querySelector(".session-user-text")!;
    expect(user.classList.contains("session-block")).toBe(true);
    expect(userText.textContent).toBe("0F\n2.");
    // The user box appears before the reasoning collapse.
    expect(
      user.nextElementSibling!.classList.contains("session-collapse"),
    ).toBe(true);
  });

  it("hides the player box when the exchange carries no message", () => {
    const c = createSessionBox(container);
    c.render({ phase: "idle", reasoning: "", content: "", user: "" });
    const user = container.querySelector(".session-user") as HTMLElement;
    expect(user.style.display).toBe("none");
    c.render({ phase: "running", reasoning: "", content: "", user: "hi" });
    expect(user.style.display).toBe("");
  });

  it("renders an image thumbnail and opens/closes the lightbox on click", () => {
    const c = createSessionBox(container);
    c.render({
      phase: "done",
      reasoning: "",
      content: "",
      user: "棋盘：下面是一张棋盘截图",
      userImageUrl: "data:image/png;base64,AAAA",
    });
    const img = container.querySelector(
      ".session-user-image",
    ) as HTMLImageElement;
    expect(img.hidden).toBe(false);
    expect(img.src).toBe("data:image/png;base64,AAAA");

    const lightbox = container.querySelector(
      ".session-lightbox",
    ) as HTMLElement;
    const lightboxImg = lightbox.querySelector("img")! as HTMLImageElement;
    expect(lightbox.style.display).toBe("none");
    img.click();
    expect(lightbox.style.display).toBe("flex");
    expect(lightboxImg.src).toBe("data:image/png;base64,AAAA");
    lightbox.click();
    expect(lightbox.style.display).toBe("none");
  });
});

describe("createSessionBox auto-scroll (issue #128)", () => {
  // The scroll container's bottom is `scrollHeight - clientHeight`. A single
  // box instance per test keeps `pinned` alive across renders.
  function scrollTo(el: HTMLElement, top: number): void {
    el.scrollTop = top;
    el.dispatchEvent(new Event("scroll"));
  }

  it("stays pinned to the bottom as content streams in", () => {
    mockScrollMetrics(container, 100, 200);
    const c = createSessionBox(container);
    c.render({ phase: "running", reasoning: "think", content: "", user: "" });
    expect(container.scrollTop).toBe(200);
    c.render({
      phase: "running",
      reasoning: "think more",
      content: "",
      user: "",
    });
    expect(container.scrollTop).toBe(200);
  });

  it("releases the lock when the user scrolls away from the bottom", () => {
    mockScrollMetrics(container, 100, 200);
    const c = createSessionBox(container);
    c.render({ phase: "running", reasoning: "think", content: "", user: "" });
    expect(container.scrollTop).toBe(200);
    // The user drags the scrollbar up; the pin is released.
    scrollTo(container, 40);
    c.render({
      phase: "running",
      reasoning: "think more",
      content: "",
      user: "",
    });
    expect(container.scrollTop).toBe(40); // stays where the user put it
  });

  it("re-engages the lock when the user drags back to the bottom", () => {
    mockScrollMetrics(container, 100, 200);
    const c = createSessionBox(container);
    // Unlock by scrolling up.
    scrollTo(container, 40);
    c.render({ phase: "running", reasoning: "think", content: "", user: "" });
    expect(container.scrollTop).toBe(40);
    // Back to the bottom: the lock re-engages.
    scrollTo(container, 100);
    c.render({
      phase: "running",
      reasoning: "think more",
      content: "",
      user: "",
    });
    expect(container.scrollTop).toBe(200);
  });

  it("re-pins on a fresh run even after the user scrolled up", () => {
    mockScrollMetrics(container, 100, 200);
    const c = createSessionBox(container);
    // A previous run ended with the user scrolled up (unlocked).
    scrollTo(container, 40);
    c.render({ phase: "done", reasoning: "old", content: "c", user: "" });
    expect(container.scrollTop).toBe(40);
    // A new analysis begins: start() emits an empty running state.
    c.render({ phase: "running", reasoning: "", content: "", user: "" });
    expect(container.scrollTop).toBe(200);
  });
});
