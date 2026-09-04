// The `AiGuide` composition (ADR-0012): the top-left game area (a full copy
// of SinglePlay with its own independent game client), the bottom-left
// dashboard (analyze/interrupt, input format, session strategy, row/col axis,
// history), and the right dialog shell. The "Analyze" button drives the
// `GuideMachine` (issue #119) which consumes the real SSE stream; the dialog is
// rendered by `createConversation`.

import type { BoardFormat, GuideRequest, ProviderError } from "../ai/api";
import { createConversation } from "../ai/conversation";
import { createBoardAxis, type AxisOverlay } from "../ai/axis";
import { createGuideMachine, type GuideState } from "../ai/stateMachine";
import { createGameArea, type GameArea } from "./gameArea";
import type { AppDeps, SessionStrategy } from "./mode";

export interface Composition {
  dispose(): void;
}

const FORMATS: ReadonlyArray<{ value: BoardFormat; label: string }> = [
  { value: "simple-text", label: "A 简单字符 (simple-text)" },
  { value: "emoji", label: "B Emoji (emoji)" },
  { value: "full-coordinates", label: "C 完整坐标 (full-coordinates)" },
  { value: "image", label: "D 图像 (image)" },
];

const STRATEGIES: ReadonlyArray<{
  value: SessionStrategy;
  label: string;
  disabled: boolean;
}> = [
  { value: "per-analysis", label: "per-analysis", disabled: false },
  { value: "per-game", label: "per-game (未实现)", disabled: true },
];

/** Buckets a `preflight-failed` error into a human alert message (issue #97 ①).
 * 4xx/5xx surface via the HTTP status / provider kind; the alert blocks
 * (synchronous `window.alert`). */
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
  let history: Array<{ format: BoardFormat; state: GuideState }> = [];
  let running = false;

  const gameArea: GameArea = createGameArea(gameZone, {
    onNewGame: () => {
      // A new game abandons the current board: reset the per-game session and
      // history (issue #114: history binds to the current game).
      history = [];
      machine.reset();
      renderHistory();
    },
  });
  // Default off (user story #16): createBoardAxis starts hidden; the checkbox
  // drives setVisible. The row/col labels themselves are #118's product.
  const axis: AxisOverlay = createBoardAxis(gameArea.boardEl);

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
  dashboard.appendChild(formatSelect);
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
  dashboard.appendChild(strategySelect);

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
    const req: GuideRequest = { format, imageDataUrl };
    machine.start(req);
  }

  analysisBtn.addEventListener("click", () => {
    if (running) void machine.interrupt_by_user();
    else void startAnalysis();
  });

  const dispose = (): void => {
    unsubscribe();
    axis.destroy();
    gameArea.dispose();
    container.remove();
  };

  return { dispose };
}
