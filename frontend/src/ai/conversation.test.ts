// @vitest-environment jsdom

// Tests for the dual-stream dialog renderer (issue #119): reasoning is
// collapsible, content is plain, SUGGEST stays text, and an interrupt renders a
// red tail line.

import { beforeEach, describe, expect, it } from "vitest";
import { createConversation } from "./conversation";

let container: HTMLElement;

beforeEach(() => {
  container = document.createElement("div");
  container.className = "dialog-stream";
  document.body.appendChild(container);
});

function render(
  state: Parameters<ReturnType<typeof createConversation>["render"]>[0],
): void {
  createConversation(container).render(state);
}

describe("createConversation", () => {
  it("renders reasoning and content into their blocks", () => {
    render({
      phase: "running",
      reasoning: "think",
      content: "SUGGEST null",
      user: "",
    });
    expect(container.querySelector(".dialog-reasoning")!.textContent).toBe(
      "think",
    );
    expect(container.querySelector(".dialog-content")!.textContent).toBe(
      "SUGGEST null",
    );
  });

  it("makes the reasoning block a collapsible details, content not", () => {
    render({ phase: "running", reasoning: "think", content: "hi", user: "" });
    // The reasoning block is inside a <details> collapsible.
    expect(container.querySelector(".dialog-collapse")).toBeTruthy();
    expect(
      container.querySelector(".dialog-reasoning")!.closest("details"),
    ).toBeTruthy();
    // Content is not wrapped in a collapsible.
    expect(
      container.querySelector(".dialog-content")!.closest("details"),
    ).toBeNull();
  });

  it("expands the reasoning block by default and keeps it open across renders", () => {
    const c = createConversation(container);
    const details = container.querySelector(
      "details.dialog-collapse",
    ) as HTMLDetailsElement;
    expect(details.open).toBe(true);

    // The user-visible `open` state is preserved when streaming updates arrive.
    c.render({ phase: "running", reasoning: "更多", content: "x", user: "" });
    expect(details.open).toBe(true);
    c.render({ phase: "done", reasoning: "结论", content: "y", user: "" });
    expect(details.open).toBe(true);
  });

  it("renders an interrupt as a red tail line, not an alert", () => {
    render({
      phase: "interrupted",
      reasoning: "r",
      content: "c",
      user: "",
      interruptReason: "user_interrupt",
    });
    expect(container.querySelector(".dialog-interrupt")!.textContent).toBe(
      "已中断:user_interrupt",
    );
  });

  it("keeps SUGGEST as plain text (no parsing, no highlighting)", () => {
    render({
      phase: "done",
      reasoning: "",
      content: 'SUGGEST {"row":2,"col":3}',
      user: "",
    });
    const content = container.querySelector(".dialog-content")!;
    expect(content.textContent).toBe('SUGGEST {"row":2,"col":3}');
    // Plain text: no child elements, no addresses parsed into markup.
    expect(content.querySelectorAll("*")).toHaveLength(0);
  });

  it("hides the empty reasoning/content/interrupt blocks", () => {
    render({ phase: "idle", reasoning: "", content: "", user: "" });
    const collapse = container.querySelector(".dialog-collapse") as HTMLElement;
    const content = container.querySelector(".dialog-content") as HTMLElement;
    const interrupt = container.querySelector(
      ".dialog-interrupt",
    ) as HTMLElement;
    expect(collapse.style.display).toBe("none");
    expect(content.style.display).toBe("none");
    expect(interrupt.style.display).toBe("none");
  });

  it("shows interleaved reasoning and content as accumulated blocks", () => {
    const c = createConversation(container);
    c.render({ phase: "running", reasoning: "思考", content: "", user: "" });
    c.render({
      phase: "running",
      reasoning: "思考中",
      content: "答案出来",
      user: "",
    });
    expect(container.querySelector(".dialog-reasoning")!.textContent).toBe(
      "思考中",
    );
    expect(container.querySelector(".dialog-content")!.textContent).toBe(
      "答案出来",
    );
  });

  it("renders the player message boxed, before the AI reasoning block", () => {
    render({
      phase: "done",
      reasoning: "think",
      content: "SUGGEST null",
      user: "Difficulty: Beginner\n0 F\n2 .",
    });
    const user = container.querySelector(".dialog-user")!;
    const userText = container.querySelector(".dialog-user-text")!;
    expect(user.classList.contains("dialog-block")).toBe(true);
    expect(userText.textContent).toBe("Difficulty: Beginner\n0 F\n2 .");
    // The user box appears before the reasoning collapse.
    expect(user.nextElementSibling!.classList.contains("dialog-collapse")).toBe(
      true,
    );
  });

  it("hides the player box when the exchange carries no message", () => {
    const c = createConversation(container);
    c.render({ phase: "idle", reasoning: "", content: "", user: "" });
    const user = container.querySelector(".dialog-user") as HTMLElement;
    expect(user.style.display).toBe("none");
    c.render({ phase: "running", reasoning: "", content: "", user: "hi" });
    expect(user.style.display).toBe("");
  });

  it("renders an image thumbnail and opens/closes the lightbox on click", () => {
    const c = createConversation(container);
    c.render({
      phase: "done",
      reasoning: "",
      content: "",
      user: "棋盘：下面是一张棋盘截图",
      userImageUrl: "data:image/png;base64,AAAA",
    });
    const img = container.querySelector(
      ".dialog-user-image",
    ) as HTMLImageElement;
    expect(img.hidden).toBe(false);
    expect(img.src).toBe("data:image/png;base64,AAAA");

    const lightbox = container.querySelector(".dialog-lightbox") as HTMLElement;
    const lightboxImg = lightbox.querySelector("img")! as HTMLImageElement;
    expect(lightbox.style.display).toBe("none");
    img.click();
    expect(lightbox.style.display).toBe("flex");
    expect(lightboxImg.src).toBe("data:image/png;base64,AAAA");
    lightbox.click();
    expect(lightbox.style.display).toBe("none");
  });
});
