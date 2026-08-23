// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import type { SessionResult } from "../api";
import { renderPanelError, renderTranscript } from "./panel";

describe("renderTranscript", () => {
  it("renders tool calls, reasoning, and content per turn", () => {
    const el = document.createElement("div");
    const result: SessionResult = {
      steps: [
        {
          reasoning_content: "先看棋盘",
          content: null,
          tool_calls: [{ name: "read_board", arguments: "{}" }],
        },
        {
          reasoning_content: "分析中",
          content: "右侧较安全",
          tool_calls: [],
        },
      ],
    };

    renderTranscript(el, result);

    expect(el.querySelectorAll(".ai-turn").length).toBe(2);
    expect(el.querySelector(".ai-tool")?.textContent).toBe(
      "调用工具 read_board",
    );
    expect(el.querySelector(".ai-reasoning")?.textContent).toBe("先看棋盘");
    expect(el.querySelector(".ai-content")?.textContent).toBe("右侧较安全");
  });
});

describe("renderPanelError", () => {
  it("shows the error message", () => {
    const el = document.createElement("div");
    renderPanelError(el, "boom");
    expect(el.querySelector(".ai-error")?.textContent).toBe("boom");
  });
});
