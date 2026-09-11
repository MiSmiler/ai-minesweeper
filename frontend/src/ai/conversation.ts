// The dual-stream dialog renderer (issue #119): `reasoning` is a light,
// smaller, whole-block collapsible; `content` is normal font and never
// collapses; the trailing `(row,col)` coordinate is plain text — never parsed,
// never highlighted (issue #95). A mid-stream interrupt renders
// as a red `已中断:<reason>` tail line. The auto-scroll respects the user's
// scrollbar (issue #128): it stays pinned to the bottom only while the user
// is not scrolling away, and releases the moment they scroll up.

import type { GuideState } from "./stateMachine";

export interface Conversation {
  render(state: GuideState): void;
}

/** Mounts the dialog into `container` and returns a renderer that updates it
 * in place. The elements are kept across renders so the reasoning block's
 * collapse/expand state survives streaming. */
export function createConversation(container: HTMLElement): Conversation {
  // The auto-scroll lock (issue #128): `pinned` is derived from the container's
  // own scroll position, so any `scroll` (user drag / wheel / keyboard, or our
  // own programmatic scroll below) recomputes it. Dragging the scrollbar away
  // releases the lock; dragging it back re-engages it.
  const SCROLL_BOTTOM_TOL = 2; // 2px: absorbs sub-pixel / scrollbar-width jitter
  let pinned = true; // a fresh dialog is pinned to the bottom
  const isAtBottom = (): boolean =>
    container.scrollHeight - container.scrollTop - container.clientHeight <=
    SCROLL_BOTTOM_TOL;
  // A render that pins to the bottom fires this with isAtBottom()===true, so
  // `pinned` stays true — no feedback loop.
  container.addEventListener("scroll", () => {
    pinned = isAtBottom();
  });

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

  // The player's message is wrapped in the only boxed turn (issue #124): a
  // deeper panel, distinct from the unboxed AI reasoning/content text.
  const userBlock = document.createElement("div");
  userBlock.className = "dialog-block dialog-user";
  const userText = document.createElement("div");
  userText.className = "dialog-user-text";
  const userImage = document.createElement("img");
  userImage.className = "dialog-user-image";
  userImage.alt = "玩家发送的棋盘截图";
  userImage.hidden = true;
  userBlock.append(userText, userImage);

  // A full-size overlay for the image form's thumbnail (issue #124). It lives
  // inside `.dialog-stream` with `position: fixed`, so it overlays the viewport
  // and is torn down with the dialog container on dispose. Visibility is driven
  // by `style.display` (not the `hidden` attribute) because the CSS
  // `display: flex` would otherwise override `hidden`.
  const lightbox = document.createElement("div");
  lightbox.className = "dialog-lightbox";
  lightbox.style.display = "none";
  const lightboxImg = document.createElement("img");
  lightbox.append(lightboxImg);
  userImage.addEventListener("click", () => {
    lightboxImg.src = userImage.src;
    lightbox.style.display = "flex";
  });
  lightbox.addEventListener("click", () => {
    lightbox.style.display = "none";
  });

  container.replaceChildren(
    userBlock,
    collapse,
    contentBlock,
    interruptBlock,
    lightbox,
  );

  const render = (state: GuideState): void => {
    // The player's boxed turn: show only when there is text or a screenshot.
    const hasUser = state.user !== "" || state.userImageUrl !== undefined;
    if (hasUser) {
      userText.textContent = state.user;
      if (state.userImageUrl !== undefined) {
        userImage.src = state.userImageUrl;
        userImage.hidden = false;
      } else {
        userImage.removeAttribute("src");
        userImage.hidden = true;
      }
      userBlock.style.display = "";
    } else {
      // A new run / mode change cleared the exchange; drop any lingering box
      // and close an open lightbox.
      userText.textContent = "";
      userImage.removeAttribute("src");
      userImage.hidden = true;
      userBlock.style.display = "none";
      lightbox.style.display = "none";
    }
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

    // A fresh run (issue #128): `start()` emits phase === "running" with both
    // streams empty — the only render shape that means "a new analysis began" —
    // so re-pin to the bottom to watch it stream. This relies on the
    // stateMachine guarantee (stateMachine.ts start()); revisit it if start()
    // ever stops emitting an empty running state.
    if (
      state.phase === "running" &&
      state.reasoning === "" &&
      state.content === ""
    ) {
      pinned = true;
    }

    // Only scroll when pinned; otherwise leave the scrollbar where the user put
    // it (issue #128). An instant jump (no smooth animation) to avoid jank under
    // token streaming.
    if (pinned) container.scrollTop = container.scrollHeight;
  };

  render({ phase: "idle", reasoning: "", content: "", user: "" });

  return { render };
}
