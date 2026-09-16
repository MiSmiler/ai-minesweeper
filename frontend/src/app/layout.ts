// The single page layout (ADR-0023): the Board on the left with the
// aux bar below it, the AI column on the right holding the
// dashboard above the SessionBox. There is one Game and one layout — the
// HumanPlayer plays with the mouse while the AiPlayer is driven by hand
// (ADR-0019), rather than two PlayModes to switch between.
//
// `mountLayout` is the app's composition root for the page: it builds the DOM,
// instantiates the game slice, the AI dashboard and the SessionBox, and wires
// the `AiPlayerMachine` to them. `main.ts` calls it once and keeps only the
// `beforeunload` guard.
//
// The controls follow the AI Session (issue #133): Send is disabled while
// there is no session, the InputMode select locks for as long as one is live
// (the mode is the Session's), and the discard confirms fire exactly when the
// session is `used`. The SessionBox itself is hidden until a Session exists.
//
// The session button carries the two lifecycle operations: "启动AI会话" is
// enabled only while the session is `none`, and "关闭AI会话" ends it — behind
// the discard confirm when it is `used`. A live Session is closed, never
// replaced in place.
//
// Send and the axis toggle are temporary manual-driver affordances (ADR-0019),
// so they sit in `.aux-bar` below the Board; the dashboard keeps
// only the session button and the two Send-strength settings, on one row.

import type {
  AiApi,
  InputMode,
  ProviderError,
  SendRequest,
  ThinkingLevel,
} from "../ai-player/api";
import { createSessionBox } from "../ai-player/sessionBox";
import { createBoardAxis, type BoardAxis } from "./boardAxis";
import {
  createAiPlayerMachine,
  type SessionState,
} from "../ai-player/stateMachine";
import { createGameArea, type GameArea } from "../game/gameArea";
import type { CaptureBoardImage } from "../ai-player/screenshot";

/** What the layout needs from outside, injected by `main.ts`. */
export interface AppDeps {
  /** The ai-player slice entry point (a stub under jsdom). */
  aiApi: AiApi;
  /** Screenshots the board for the image InputMode; a stub under jsdom,
   * since the browser-only capture never runs there. */
  captureBoardImage: CaptureBoardImage;
}

/** The mounted page, with the two handles `main.ts` needs. */
export interface LayoutHandle {
  /** Tears down the listeners, the timer poll, the AI Session and the DOM. */
  dispose(): void;
  /** True once the player has sent something into the live Session — the
   * `used` half of the session's `unused` / `used` predicate, read by
   * `main.ts`'s beforeunload guard. */
  hasUsedSession(): boolean;
}

const MODES: ReadonlyArray<{ value: InputMode; label: string }> = [
  { value: "plain", label: "plain" },
  { value: "emoji", label: "emoji" },
  { value: "image", label: "image" },
];

const LEVELS: ReadonlyArray<{ value: ThinkingLevel; label: string }> = [
  { value: "off", label: "off" },
  { value: "low", label: "low" },
  { value: "high", label: "high" },
  { value: "max", label: "max" },
];

/** Wraps a dropdown with a left-side caption explaining what it does. */
function labeledField(caption: string, control: HTMLElement): HTMLElement {
  const row = document.createElement("label");
  row.className = "field-row";
  const text = document.createElement("span");
  text.className = "ai-dash-label";
  text.textContent = caption;
  row.append(text, control);
  return row;
}

/** Buckets a provider failure into a human alert message
 * (issue #97 ①). 4xx/5xx surface via the HTTP status / provider kind; the alert
 * blocks (synchronous `window.alert`).
 *
 * This is the consumer of the `kind` field on the backend
 * `ProviderError` body (issue #123): `config` ("AI 未配置") and `upstream`
 * ("AI 服务异常") must stay distinct, and they are *not* derivable from `code`
 * when the error carries no HTTP status (no provider / bad key / transport
 * failure all have `code: null`). Do not remove the `kind` field. */
function providerAlertMessage(e: ProviderError): string {
  if (e.kind === "config") return `AI 未配置：${e.message}`;
  if (e.code === 429) return "AI 请求过于频繁（429），请稍后再试。";
  if (e.code === 408) return "AI 响应超时（408），请稍后再试。";
  if (e.kind === "upstream") return `AI 服务异常：${e.message}`;
  return `发送失败：${e.message}`;
}

/** Mounts the page (`.layout` = `.game-column` + `.ai-column`) into `root`. */
export function mountLayout(root: HTMLElement, deps: AppDeps): LayoutHandle {
  const layout = document.createElement("div");
  layout.className = "layout";
  root.replaceChildren(layout);

  // --- Left: the board column (an independent game body + its axis labels) ---
  const gameColumn = document.createElement("div");
  gameColumn.className = "game-column";
  layout.appendChild(gameColumn);

  let currentMode: InputMode = "plain";
  // The reasoning depth (issue #122): default low, session-persistent, and
  // independent of the input mode — changing it never invalidates a session.
  let currentLevel: ThinkingLevel = "low";
  let running = false;
  // Mirrors the machine's `sessionState` so the synchronous predicates
  // (`beforeNewGame`) can read it without a subscription.
  let sessionState: SessionState = "none";
  // True while `machine.begin()` is in flight. The session stays `none` until
  // it lands, so the session button would otherwise read "启动AI会话" and take
  // a second click. Local to the layout: `startSession` is the only caller.
  let beginPending = false;
  // The axis overlay needs `boardEl`, so it is created after the game body;
  // `onRender` may fire before the assignment below completes, but it only
  // fires once the initial snapshot loads asynchronously, by which time the
  // `createBoardAxis` call has run.
  let axis: BoardAxis | null = null;

  const gameArea: GameArea = createGameArea(gameColumn, {
    onNewGame: () => {
      // A new game ends the AI Session (the backend's new-game action already
      // ended it, issue #133).
      machine.end();
    },
    // A new game discards a used AI Session; ask first exactly when the
    // session is `used` (issue #133). Pressing New Game during an in-flight
    // Send interrupts it first.
    beforeNewGame: () => {
      if (
        sessionState === "used" &&
        !window.confirm("开始新游戏将结束当前 AI 会话，是否继续？")
      ) {
        return false;
      }
      if (running) void machine.interrupt_by_user();
      return true;
    },
    // Render the 0-based row/col labels for the live Board. The
    // axis is pure DOM and sits outside the Board, so it never affects the 4
    // AI input forms.
    onRender: (snapshot) => axis?.setRowsCols(snapshot.rows, snapshot.cols),
  });
  // Default off (user story #16): createBoardAxis starts hidden; the checkbox
  // drives setVisible.
  axis = createBoardAxis(gameArea.boardEl);

  // The aux bar: the controls a human uses to drive the AiPlayer by hand.
  // Both are temporary — the Send goes when the tool loop lands — so they sit
  // below the Board instead of in the dashboard.
  const auxBar = document.createElement("div");
  auxBar.className = "aux-bar";
  gameColumn.appendChild(auxBar);

  // --- Right: the AI column (dashboard above the SessionBox) ---
  const aiColumn = document.createElement("div");
  aiColumn.className = "ai-column";
  layout.appendChild(aiColumn);

  const dashboard = document.createElement("div");
  dashboard.className = "ai-dashboard";
  aiColumn.appendChild(dashboard);

  // The dashboard row: the session button and the two Send-strength settings
  // side by side.
  const dashRow = document.createElement("div");
  dashRow.className = "dashboard-row";
  dashboard.appendChild(dashRow);

  // Send / interrupt button (dual state, user story #34); mounted in the
  // aux bar below the Board, not here.
  const sendBtn = document.createElement("button");
  sendBtn.type = "button";
  sendBtn.className = "send-btn";
  sendBtn.textContent = "发送";
  sendBtn.disabled = true; // no AI Session yet

  // Its two faces ("启动AI会话" / "关闭AI会话") are drawn by `syncSessionBtn`.
  const sessionBtn = document.createElement("button");
  sessionBtn.type = "button";
  sessionBtn.className = "session-btn";

  dashRow.appendChild(sessionBtn);

  // Input-mode dropdown (3 modes, user story #20/#21).
  const modeSelect = document.createElement("select");
  modeSelect.className = "input-mode-select";
  for (const m of MODES) {
    const opt = document.createElement("option");
    opt.value = m.value;
    opt.textContent = m.label;
    modeSelect.appendChild(opt);
  }
  dashRow.appendChild(labeledField("输入模式", modeSelect));
  modeSelect.addEventListener("change", () => {
    const next = modeSelect.value as InputMode;
    if (next === currentMode) return;
    // The select is locked for as long as a Session is live, so a change
    // happens only while there is none.
    currentMode = next;
  });

  // Thinking-level dropdown (issue #122: off/low/high/max, default low). The
  // level is a Send-strength setting, not a board view — changing it never
  // invalidates a session (unlike the mode select).
  const levelSelect = document.createElement("select");
  levelSelect.className = "level-select";
  for (const l of LEVELS) {
    const opt = document.createElement("option");
    opt.value = l.value;
    opt.textContent = l.label;
    levelSelect.appendChild(opt);
  }
  levelSelect.value = currentLevel;
  dashRow.appendChild(labeledField("思考深度", levelSelect));
  levelSelect.addEventListener("change", () => {
    currentLevel = levelSelect.value as ThinkingLevel;
  });

  // Row/col axis checkbox (user story #16–#19).
  const axisCheckbox = document.createElement("input");
  axisCheckbox.type = "checkbox";
  axisCheckbox.className = "axis-checkbox";
  const axisToggle = document.createElement("label");
  axisToggle.className = "axis-toggle";
  axisToggle.append(axisCheckbox, document.createTextNode("行列号"));
  // Axis toggle left, Send right; the bar hugs its content and sits at the
  // column's right edge.
  auxBar.append(axisToggle, sendBtn);
  axisCheckbox.addEventListener("change", () => {
    axis.setVisible(axisCheckbox.checked);
  });

  // --- Right, below the dashboard: the SessionBox shell ---
  const boxEl = document.createElement("div");
  boxEl.className = "ai-session-box";
  // Hidden until a Session exists (ADR-0023): the box exists to show a
  // Session, so `sessionState !== "none"` is the visibility rule.
  boxEl.style.display = "none";
  const boxTitle = document.createElement("h3");
  boxTitle.textContent = "AI 会话";
  const streamEl = document.createElement("div");
  streamEl.className = "session-stream";
  boxEl.append(boxTitle, streamEl);
  aiColumn.appendChild(boxEl);

  const sessionBox = createSessionBox(streamEl);
  const machine = createAiPlayerMachine({ api: deps.aiApi });

  function setRunning(next: boolean): void {
    running = next;
    sendBtn.textContent = next ? "中断" : "发送";
    sendBtn.classList.toggle("running", next);
  }

  /** The session button's two faces: the text follows the live Session, the
   * disabled state follows the pending `begin()`. Single writer of both. */
  function syncSessionBtn(): void {
    const live = sessionState !== "none";
    sessionBtn.textContent = live ? "关闭AI会话" : "启动AI会话";
    sessionBtn.disabled = beginPending;
  }

  syncSessionBtn(); // the button starts in its start face

  const unsubscribe = machine.onState((state) => {
    sessionBox.render(state);
    // A Session is live exactly while `sessionState !== "none"`; New Game
    // (`machine.end()`) drops back to `none` and hides the box again.
    boxEl.style.display = state.sessionState === "none" ? "none" : "";
    sessionState = state.sessionState;
    syncSessionBtn();
    setRunning(state.phase === "running");
    // The InputMode belongs to the Session: the player picks it at 启动AI会话 and
    // can change it only by closing the Session.
    sendBtn.disabled = state.sessionState === "none";
    modeSelect.disabled = state.sessionState !== "none";
    // Only a provider failure alerts; a refusal (NoSession / Busy / the
    // InputMode lock) is reported by the disabled controls, not an alert.
    if (state.phase === "failed" && state.failure?.kind === "provider") {
      window.alert(providerAlertMessage(state.failure.error));
    }
  });

  async function startSend(): Promise<void> {
    if (running || sessionState === "none") return;
    // The Session's InputMode is `currentMode`: the select is locked for as
    // long as a Session is live, so it still holds the value `begin` was given.
    let imageDataUrl: string | undefined;
    if (currentMode === "image") {
      try {
        imageDataUrl = await deps.captureBoardImage(gameArea.boardEl, {
          pixelRatio: 1,
        });
      } catch (err) {
        const message = err instanceof Error ? err.message : err;
        window.alert(`截图失败：${message}`);
        return;
      }
    }
    const req: SendRequest = {
      thinkingLevel: currentLevel,
      imageDataUrl,
    };
    machine.send(req);
  }

  /** The start face: only from `none`, since a live Session is closed rather
   * than replaced. The pending flag keeps a second click from beginning a
   * second Session while the first `begin()` is still in flight. */
  async function startSession(): Promise<void> {
    if (beginPending || sessionState !== "none") return;
    beginPending = true;
    syncSessionBtn();
    try {
      await machine.begin(currentMode);
    } finally {
      beginPending = false;
      syncSessionBtn();
    }
  }

  /** The close face: ending a used Session discards its history, so ask first
   * (the same predicate as the InputMode lock, issue #133); an in-flight Send
   * is interrupted first. */
  async function closeSession(): Promise<void> {
    if (
      sessionState === "used" &&
      !window.confirm("关闭AI会话将结束当前会话，是否继续？")
    ) {
      return;
    }
    if (running) await machine.interrupt_by_user();
    machine.end();
  }

  sendBtn.addEventListener("click", () => {
    if (running) void machine.interrupt_by_user();
    else void startSend();
  });
  sessionBtn.addEventListener("click", () => {
    if (sessionState === "none") void startSession();
    else void closeSession();
  });

  const dispose = (): void => {
    unsubscribe();
    machine.end();
    axis?.destroy();
    gameArea.dispose();
    layout.remove();
  };

  return {
    dispose,
    /** True once the Session holds messages a refresh would clear. */
    hasUsedSession: () => sessionState === "used",
  };
}
