// The `AiGuide` composition (ADR-0012): the top-left game area (a full copy
// of SinglePlay with its own independent game client), the bottom-left
// dashboard (analyze/interrupt, input format, session strategy, row/col axis,
// history), and the right dialog shell. The "Analyze" button drives the
// `GuideMachine` (issue #119) which consumes the real SSE stream; the dialog is
// rendered by `createConversation`.

import type {
  BoardFormat,
  GuideRequest,
  ProviderError,
  ThinkingLevel,
} from "../ai/api";
import { createConversation } from "../ai/conversation";
import { createBoardAxis, type AxisOverlay } from "../ai/axis";
import { createGuideMachine, type GuideState } from "../ai/stateMachine";
import { createGameArea, type GameArea } from "./gameArea";
import type { AppDeps, Composition, SessionStrategy } from "./mode";

const FORMATS: ReadonlyArray<{ value: BoardFormat; label: string }> = [
  { value: "simple-text", label: "A 简单字符 (simple-text)" },
  { value: "emoji", label: "B Emoji (emoji)" },
  { value: "full-coordinates", label: "C 完整坐标 (full-coordinates)" },
  { value: "image", label: "D 图像 (image)" },
];

const LEVELS: ReadonlyArray<{ value: ThinkingLevel; label: string }> = [
  { value: "off", label: "off" },
  { value: "low", label: "low" },
  { value: "high", label: "high" },
  { value: "max", label: "max" },
];

const STRATEGIES: ReadonlyArray<{
  value: SessionStrategy;
  label: string;
  disabled: boolean;
}> = [
  { value: "per-analysis", label: "per-analysis", disabled: false },
  { value: "per-game", label: "per-game (未实现)", disabled: true },
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

  let currentFormat: BoardFormat = "simple-text";
  // The reasoning depth (issue #122): default low, session-persistent, and
  // independent of format — changing it never clears the guide history.
  let currentLevel: ThinkingLevel = "low";
  let history: Array<{ format: BoardFormat; state: GuideState }> = [];
  let running = false;
  // The axis overlay needs `boardEl`, so it is created after the game area;
  // `onRender` may fire before the assignment below completes, but it only
  // fires once the initial snapshot loads asynchronously, by which time the
  // `createBoardAxis` call has run (issue #118).
  let axis: AxisOverlay | null = null;

  const gameArea: GameArea = createGameArea(gameZone, {
    onNewGame: () => {
      // A new game abandons the current board: reset the per-game session and
      // history (issue #114: history binds to the current game).
      history = [];
      machine.reset();
      renderHistory();
    },
    // Any new game (smiley or difficulty) discards the per-game history; ask
    // first when there is history to lose (issue #112 US-32 spirit).
    beforeNewGame: () =>
      history.length === 0 ||
      window.confirm("开始新游戏将清空 guide 历史，是否继续？"),
    // Render the 0-based row/col labels for the live Board (issue #118). The
    // axis is pure DOM and sits outside the Board, so it never affects the 4
    // AI input forms.
    onRender: (snapshot) => axis?.setRowsCols(snapshot.rows, snapshot.cols),
  });
  // Default off (user story #16): createBoardAxis starts hidden; the checkbox
  // drives setVisible. The 0-based row/col labels are #118's product — rendered
  // here (backed by the same axis above) once the board loads.
  axis = createBoardAxis(gameArea.boardEl);

  /** A fresh session id per analysis; the backend only needs uniqueness. */
  function newSessionId(): string {
    return `session-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }

  // --- Bottom-left dashboard ---
  const dashboard = document.createElement("div");
  dashboard.className = "guide-dashboard";
  left.appendChild(dashboard);

  // Analyze / interrupt button (dual state, user story #34).
  const analysisBtn = document.createElement("button");
  analysisBtn.type = "button";
  analysisBtn.className = "analysis-btn";
  analysisBtn.textContent = "分析";
  dashboard.appendChild(analysisBtn);

  // Input-format dropdown (4 forms, user story #20/#21).
  const formatSelect = document.createElement("select");
  formatSelect.className = "format-select";
  for (const f of FORMATS) {
    const opt = document.createElement("option");
    opt.value = f.value;
    opt.textContent = f.label;
    formatSelect.appendChild(opt);
  }
  dashboard.appendChild(labeledField("输入格式", formatSelect));
  formatSelect.addEventListener("change", () => {
    const next = formatSelect.value as BoardFormat;
    if (next === currentFormat) return;
    // Changing format invalidates old analyses: confirm + clear (user story
    // #32; the decision lives in the assembly layer).
    if (history.length > 0) {
      if (!window.confirm("更改输入格式将清空历史，是否继续？")) {
        formatSelect.value = currentFormat; // decline: revert the selection
        return;
      }
    }
    currentFormat = next;
    history = [];
    machine.reset();
    renderHistory();
  });

  // Thinking-level dropdown (issue #122: off/low/high/max, default low). The
  // level is an analysis-strength setting, not a board view — changing it never
  // invalidates or clears the guide history (unlike the format select).
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

  // Session-strategy dropdown (user story / issue #96: per-analysis usable,
  // per-game greyed and labelled "not implemented").
  const strategySelect = document.createElement("select");
  strategySelect.className = "strategy-select";
  for (const s of STRATEGIES) {
    const opt = document.createElement("option");
    opt.value = s.value;
    opt.textContent = s.label;
    opt.disabled = s.disabled;
    strategySelect.appendChild(opt);
  }
  dashboard.appendChild(labeledField("会话策略", strategySelect));

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
  const machine = createGuideMachine({ api: deps.aiApi, newSessionId });

  function setRunning(next: boolean): void {
    running = next;
    analysisBtn.textContent = next ? "中断" : "分析";
    analysisBtn.classList.toggle("running", next);
    historyList.classList.toggle("locked", next);
  }

  const unsubscribe = machine.onState((state) => {
    conversation.render(state);
    setRunning(state.phase === "running");
    // A completed analysis is recorded in history; interrupted / pre-flight
    // failures are not (partial / absent output, issue #97).
    if (state.phase === "done") {
      history.push({ format: currentFormat, state: { ...state } });
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
      li.textContent = `分析 #${i + 1} (${entry.format})`;
      li.addEventListener("click", () => {
        if (running) return; // Not clickable while an analysis is running (user story #31)
        conversation.render(entry.state);
      });
      historyList.appendChild(li);
    });
  }
  renderHistory();

  async function startAnalysis(): Promise<void> {
    if (running) return;
    const format = currentFormat;
    let imageDataUrl: string | undefined;
    if (format === "image") {
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
    const req: GuideRequest = {
      format,
      thinkingLevel: currentLevel,
      imageDataUrl,
    };
    machine.start(req);
  }

  analysisBtn.addEventListener("click", () => {
    if (running) void machine.interrupt_by_user();
    else void startAnalysis();
  });

  const dispose = (): void => {
    unsubscribe();
    axis?.destroy();
    gameArea.dispose();
    container.remove();
  };

  return {
    dispose,
    /** True while the guide holds analyses a refresh / mode switch would clear. */
    hasGuideHistory: () => history.length > 0,
    /** Blocking confirm before discarding guide history (mode switch). */
    confirmDiscard: (message) =>
      history.length === 0 || window.confirm(message),
  };
}
