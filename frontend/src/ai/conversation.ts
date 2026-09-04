// The dual-stream dialog renderer (issue #119): `reasoning` is a light,
// smaller, whole-block collapsible; `content` is normal font and never
// collapses; `SUGGEST {"row":N,"col":M}` / `SUGGEST null` are plain text —
// never parsed, never highlighted (issue #95). A mid-stream interrupt renders
// as a red `已中断:<reason>` tail line.

import type { GuideState } from "./stateMachine";

export interface Conversation {
  render(state: GuideState): void;
}

/** Mounts the dialog into `container` and returns a renderer that updates it
 * in place. The elements are kept across renders so the reasoning block's
 * collapse/expand state survives streaming. */
export function createConversation(container: HTMLElement): Conversation {
  // Reasoning: the whole block is one collapsible (<details>) whose body holds
  // the text. The `.dialog-reasoning` class stays on the text element so the
  // existing styling/test selectors keep working.
  const collapse = document.createElement("details");
  collapse.className = "dialog-collapse";
  // The reasoning block is expanded by default (user story #10); the user may
  // collapse it via the <summary> toggle, which `render` leaves untouched.
  collapse.open = true;
  const summary = document.createElement("summary");
  summary.className = "dialog-reasoning-summary";
  summary.textContent = "推理";
  const reasoningBlock = document.createElement("div");
  reasoningBlock.className = "dialog-block dialog-reasoning";
  collapse.append(summary, reasoningBlock);

  const contentBlock = document.createElement("div");
  contentBlock.className = "dialog-block dialog-content";

  const interruptBlock = document.createElement("div");
  interruptBlock.className = "dialog-interrupt";

  container.replaceChildren(collapse, contentBlock, interruptBlock);

  const render = (state: GuideState): void => {
    if (state.reasoning) {
      reasoningBlock.textContent = state.reasoning;
      collapse.style.display = "";
    } else {
      reasoningBlock.textContent = "";
      collapse.style.display = "none";
    }
    if (state.content) {
      contentBlock.textContent = state.content;
      contentBlock.style.display = "";
    } else {
      contentBlock.textContent = "";
      contentBlock.style.display = "none";
    }
    if (state.phase === "interrupted" && state.interruptReason) {
      interruptBlock.textContent = `已中断:${state.interruptReason}`;
      interruptBlock.style.display = "";
    } else {
      interruptBlock.textContent = "";
      interruptBlock.style.display = "none";
    }
    container.scrollTop = container.scrollHeight;
  };

  render({ phase: "idle", reasoning: "", content: "" });

  return { render };
}
