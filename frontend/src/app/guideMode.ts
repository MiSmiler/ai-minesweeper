// The `AiGuide` composition (ADR-0012): the top-left game area (a full copy
// of SinglePlay with its own independent game client), the bottom-left
// dashboard (send/interrupt, new session, input mode, row/col axis, history),
// and the right dialog shell. The "发送" button drives the `GuideMachine`
// (issue #119) which consumes the real SSE stream; the dialog is rendered by
// `createConversation`.
//
// The dashboard follows the AI Session (issue #133): Send is disabled while
// there is no session, the InputMode select locks while a Send runs / the
// session is non-empty, and the discard confirms fire exactly when the session
// is `non-empty`.

import type {
  InputMode,
  ProviderError,
  SendRequest,
  ThinkingLevel,
} from "../ai/api";
import { createConversation } from "../ai/conversation";
import { createBoardAxis, type AxisOverlay } from "../ai/axis";
import {
  createGuideMachine,
  type GuideState,
  type SessionState,
} from "../ai/stateMachine";
import { createGameArea, type GameArea } from "./gameArea";
import type { AppDeps, Composition } from "./mode";

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
  text.className = "guide-dash-label";
  text.textContent = caption;
  row.append(text, control);
  return row;
}

/** Buckets a `preflight-failed` error into a human alert message (issue #97 ①).
 * 4xx/5xx surface via the HTTP status / provider kind; the alert blocks
 * (synchronous `window.alert`).
 *
 * This is the pre-flight consumer of the `kind` field on the backend
 * `ProviderError` body (issue #123): `config` ("AI 未配置") and `upstream`
 * ("AI 服务异常") must stay distinct, and they are *not* derivable from `code`
 * when the error carries no HTTP status (no provider / bad key / transport
 * failure all have `code: null`). Do not remove the `kind` field. */
function providerAlertMessage(e: ProviderError): string {
  if (e.kind === "config") return `AI 未配置：${e.message}`;
  if (e.code === 429) return "AI 请求过于频繁（429），请稍后再试。";
  if (e.code === 408) return "AI 响应超时（408），请稍后再试。";
  if (e.kind === "upstream") return `AI 服务异常：${e.message}`;
  return `分析失败：${e.message}`;
}

/** Mounts the AiGuide composition (game area + dashboard + dialog) into `root`. */
export function composeGuideMode(
  root: HTMLElement,
  deps: AppDeps,
): Composition {
  const container = document.createElement("div");
  container.className = "guide-layout";
  root.replaceChildren(container);

  // The game + dashboard stack into one grid cell (`.guide-left`) so the dialog
  // (a sibling cell) growing never shifts them (issue #119).
  const left = document.createElement("div");
  left.className = "guide-left";
  container.appendChild(left);

  // --- Top-left game area: an independent game area + its axis labels ---
  const gameZone = document.createElement("div");
  gameZone.className = "guide-game";
  left.appendChild(gameZone);

  let currentMode: InputMode = "plain";
  // The reasoning depth (issue #122): default low, session-persistent, and
  // independent of the input mode — changing it never invalidates a session.
  let currentLevel: ThinkingLevel = "low";
  let history: Array<{ mode: InputMode; state: GuideState }> = [];
  let running = false;
  // Mirrors the machine's `sessionState` so the synchronous predicates
  // (`beforeNewGame`, `confirmDiscard`) can read it without a subscription.
  let sessionState: SessionState = "none";
  // The axis overlay needs `boardEl`, so it is created after the game area;
  // `onRender` may fire before the assignment below completes, but it only
  // fires once the initial snapshot loads asynchronously, by which time the
  // `createBoardAxis` call has run (issue #118).
  let axis: AxisOverlay | null = null;

  const gameArea: GameArea = createGameArea(gameZone, {
    onNewGame: () => {
      // A new game ends the AI Session (the backend's new-game action already
      // ended it); the history list is per-game and resets with it (issue #133).
      history = [];
      machine.endSession();
      renderHistory();
    },
    // A new game discards a non-empty AI Session; ask first exactly when the
    // session is non-empty (issue #133). Pressing New Game during an in-flight
    // Send interrupts it first.
    beforeNewGame: () => {
      if (
        sessionState === "non-empty" &&
        !window.confirm("开始新游戏将结束当前 AI 会话，是否继续？")
      ) {
        return false;
      }
      if (running) void machine.interrupt_by_user();
      return true;
    },
    // Render the 0-based row/col labels for the live Board (issue #118). The
    // axis is pure DOM and sits outside the Board, so it never affects the 4
    // AI input forms.
    onRender: (snapshot) => axis?.setRowsCols(snapshot.rows, snapshot.cols),
  });
  // Default off (user story #16): createBoardAxis starts hidden; the checkbox
  // drives setVisible. The 0-based row/col labels are #118's product — rendered
  // here (backed by the same axis above) once the board loads.
  axis = createBoardAxis(gameArea.boardEl);

  // --- Bottom-left dashboard ---
  const dashboard = document.createElement("div");
  dashboard.className = "guide-dashboard";
  left.appendChild(dashboard);

  // Send / interrupt button (dual state, user story #34) next to the new
  // session button (issue #133).
  const buttonRow = document.createElement("div");
  buttonRow.className = "button-row";
  dashboard.appendChild(buttonRow);

  const sendBtn = document.createElement("button");
  sendBtn.type = "button";
  sendBtn.className = "send-btn";
  sendBtn.textContent = "发送";
  sendBtn.disabled = true; // no AI Session yet
  buttonRow.appendChild(sendBtn);

  const newSessionBtn = document.createElement("button");
  newSessionBtn.type = "button";
  newSessionBtn.className = "new-session-btn";
  newSessionBtn.textContent = "新建AI会话";
  buttonRow.appendChild(newSessionBtn);

  // Input-mode dropdown (3 modes, user story #20/#21).
  const modeSelect = document.createElement("select");
  modeSelect.className = "input-mode-select";
  for (const m of MODES) {
    const opt = document.createElement("option");
    opt.value = m.value;
    opt.textContent = m.label;
    modeSelect.appendChild(opt);
  }
  dashboard.appendChild(labeledField("输入模式", modeSelect));
  modeSelect.addEventListener("change", () => {
    const next = modeSelect.value as InputMode;
    if (next === currentMode) return;
    // The select is locked once a Send runs / commits, so a change happens
    // only while the session is empty and the history list is already empty.
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
  dashboard.appendChild(labeledField("思考深度", levelSelect));
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
  dashboard.appendChild(axisToggle);
  axisCheckbox.addEventListener("change", () => {
    axis.setVisible(axisCheckbox.checked);
  });

  // History list (empty for a fresh game).
  const historyBox = document.createElement("div");
  historyBox.className = "history";
  const historyTitle = document.createElement("h3");
  historyTitle.textContent = "历史";
  const historyList = document.createElement("ul");
  historyList.className = "history-list";
  historyBox.append(historyTitle, historyList);
  dashboard.appendChild(historyBox);

  // --- Right dialog shell ---
  const dialog = document.createElement("div");
  dialog.className = "guide-dialog";
  const dialogTitle = document.createElement("h3");
  dialogTitle.textContent = "AI 对话";
  const dialogStream = document.createElement("div");
  dialogStream.className = "dialog-stream";
  dialog.append(dialogTitle, dialogStream);
  container.appendChild(dialog);

  const conversation = createConversation(dialogStream);
  const machine = createGuideMachine({ api: deps.aiApi });

  function setRunning(next: boolean): void {
    running = next;
    sendBtn.textContent = next ? "中断" : "发送";
    sendBtn.classList.toggle("running", next);
    historyList.classList.toggle("locked", next);
  }

  const unsubscribe = machine.onState((state) => {
    conversation.render(state);
    sessionState = state.sessionState;
    setRunning(state.phase === "running");
    // The InputMode is bound by the first committed Send: lock the select
    // while a Send runs or the session is non-empty (issue #133).
    sendBtn.disabled = state.sessionState === "none";
    modeSelect.disabled =
      state.sessionState === "non-empty" || state.phase === "running";
    // A completed Send is recorded in history; interrupted / pre-flight
    // failures are not (partial / absent output, issue #97).
    if (state.phase === "done") {
      history.push({ mode: currentMode, state: { ...state } });
      renderHistory();
    }
    if (state.phase === "preflight-failed" && state.providerError) {
      window.alert(providerAlertMessage(state.providerError));
    }
  });

  function renderHistory(): void {
    historyList.replaceChildren();
    if (history.length === 0) {
      const empty = document.createElement("li");
      empty.className = "history-empty";
      empty.textContent = "暂无";
      historyList.appendChild(empty);
      return;
    }
    history.forEach((entry, i) => {
      const li = document.createElement("li");
      li.className = "history-entry";
      li.dataset.index = String(i);
      li.textContent = `分析 #${i + 1} (${entry.mode})`;
      li.addEventListener("click", () => {
        if (running) return; // Not clickable while a Send is running (user story #31)
        conversation.render(entry.state);
      });
      historyList.appendChild(li);
    });
  }
  renderHistory();

  async function startAnalysis(): Promise<void> {
    if (running || sessionState === "none") return;
    const mode = currentMode;
    let imageDataUrl: string | undefined;
    if (mode === "image") {
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
      inputMode: mode,
      thinkingLevel: currentLevel,
      imageDataUrl,
    };
    machine.send(req);
  }

  async function startNewSession(): Promise<void> {
    // Replacing a non-empty session discards its Turns; ask first (the same
    // predicate as the InputMode lock, issue #133).
    if (
      sessionState === "non-empty" &&
      !window.confirm("新建AI会话将结束当前会话，是否继续？")
    ) {
      return;
    }
    // Pressing new session during an in-flight Send interrupts it first.
    if (running) await machine.interrupt_by_user();
    history = [];
    renderHistory();
    await machine.newSession();
  }

  sendBtn.addEventListener("click", () => {
    if (running) void machine.interrupt_by_user();
    else void startAnalysis();
  });
  newSessionBtn.addEventListener("click", () => {
    void startNewSession();
  });

  const dispose = (): void => {
    unsubscribe();
    machine.endSession();
    axis?.destroy();
    gameArea.dispose();
    container.remove();
  };

  return {
    dispose,
    /** True while the AI Session holds Turns a refresh / mode switch would clear. */
    hasGuideHistory: () => sessionState === "non-empty",
    /** Blocking confirm before discarding a non-empty AI Session (mode switch). */
    confirmDiscard: (message) =>
      sessionState !== "non-empty" || window.confirm(message),
  };
}
