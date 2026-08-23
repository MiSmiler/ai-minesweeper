import type { SessionResult } from "../api";

/** Renders the session transcript into the panel body: one card per turn,
 * showing its tool calls (the model's decisions), reasoning, and content. */
export function renderTranscript(
  container: HTMLElement,
  result: SessionResult,
): void {
  const frag = document.createDocumentFragment();
  for (const step of result.steps) {
    const card = document.createElement("div");
    card.className = "ai-turn";

    for (const call of step.tool_calls) {
      const tool = document.createElement("div");
      tool.className = "ai-tool";
      tool.textContent = `调用工具 ${call.name}`;
      card.appendChild(tool);
    }
    if (step.reasoning_content) {
      const reasoning = document.createElement("div");
      reasoning.className = "ai-reasoning";
      reasoning.textContent = step.reasoning_content;
      card.appendChild(reasoning);
    }
    if (step.content) {
      const content = document.createElement("div");
      content.className = "ai-content";
      content.textContent = step.content;
      card.appendChild(content);
    }

    frag.appendChild(card);
  }
  container.replaceChildren(frag);
}

/** Renders an error message into the panel body. */
export function renderPanelError(
  container: HTMLElement,
  message: string,
): void {
  container.replaceChildren();
  const error = document.createElement("p");
  error.className = "ai-error";
  error.textContent = message;
  container.appendChild(error);
}
