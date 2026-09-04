// The `AiGuide` composition (ADR-0012): the top-left game area (a full copy
// of SinglePlay with its own independent game client), the bottom-left
// dashboard (analyze/interrupt, input format, session strategy, row/col axis,
// history), and the right dialog shell. The "Analyze" button is wired to the
// injected `deps.aiApi`, which is a stub/mock in this ticket — the real SSE
// consumer lands later.

import type { BoardFormat, GuideEvent, ProviderError } from "../ai/api";
import { createBoardAxis, type AxisOverlay } from "../ai/axis";
import { createGameArea, type GameArea } from "./gameArea";
import type { AppDeps, SessionStrategy } from "./mode";

export interface Composition {
  dispose(): void;
}

interface HistoryEntry {
  format: BoardFormat;
  events: GuideEvent[];
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

/** Mounts the AiGuide composition (game area + dashboard + dialog) into `root`. */
export function composeGuideMode(
  root: HTMLElement,
  deps: AppDeps,
): Composition {
  const container = document.createElement("div");
  container.className = "guide-layout";
  root.replaceChildren(container);

  // --- Top-left game area: an independent game area + its axis labels ---
  const gameZone = document.createElement("div");
  gameZone.className = "guide-game";
  container.appendChild(gameZone);

  let sessionId = newSessionId();
  let currentFormat: BoardFormat = "simple-text";
  let analysisRunning = false;
  let history: HistoryEntry[] = [];
  let activeEvents: GuideEvent[] = [];

  const gameArea: GameArea = createGameArea(gameZone, {
    onNewGame: () => {
      // A new game abandons the current board: reset the per-game session
      // and history (issue #114: history binds to the current game).
      sessionId = newSessionId();
      history = [];
      activeEvents = [];
      renderHistory();
    },
  });
  // Default off (user story #16): createBoardAxis starts hidden; the checkbox
  // drives setVisible. The row/col labels themselves are #118's product.
  const axis: AxisOverlay = createBoardAxis(gameArea.boardEl);

  /** A fresh session id per game; the stub backend only needs uniqueness. */
  function newSessionId(): string {
    return `session-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }

  // --- Bottom-left dashboard ---
  const dashboard = document.createElement("div");
  dashboard.className = "guide-dashboard";
  container.appendChild(dashboard);

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
        if (analysisRunning) return; // Not clickable while an analysis is running (user story #31)
        replayHistory(entry);
      });
      historyList.appendChild(li);
    });
  }
  renderHistory();

  // --- Right dialog shell ---
  const dialog = document.createElement("div");
  dialog.className = "guide-dialog";
  const dialogTitle = document.createElement("h3");
  dialogTitle.textContent = "AI 对话";
  const dialogStream = document.createElement("div");
  dialogStream.className = "dialog-stream";
  dialog.append(dialogTitle, dialogStream);
  container.appendChild(dialog);

  function setAnalysisRunning(running: boolean): void {
    analysisRunning = running;
    analysisBtn.textContent = running ? "中断" : "分析";
    analysisBtn.classList.toggle("running", running);
    historyList.classList.toggle("locked", running);
  }

  function replayHistory(entry: HistoryEntry): void {
    dialogStream.replaceChildren();
    for (const e of entry.events) appendGuideEvent(e, false);
  }

  function appendGuideEvent(e: GuideEvent, collect: boolean): void {
    if (collect) activeEvents.push(e);
    if (e.kind === "reasoning") {
      appendDialog("reasoning", e.text);
    } else if (e.kind === "content") {
      appendDialog("content", e.text);
    } else if (e.kind === "interrupt") {
      appendDialog("interrupt", `已中断:${e.reason}`);
    }
  }

  function appendDialog(
    kind: "reasoning" | "content" | "interrupt",
    text: string,
  ): void {
    const block = document.createElement("div");
    block.className = `dialog-block dialog-${kind}`;
    block.textContent = text;
    dialogStream.appendChild(block);
    dialogStream.scrollTop = dialogStream.scrollHeight;
  }

  async function startAnalysis(): Promise<void> {
    if (analysisRunning) return;
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
    activeEvents = [];
    setAnalysisRunning(true);
    deps.aiApi.startGuide(
      sessionId,
      { format, imageDataUrl },
      onGuideEvent,
      onProviderError,
    );
  }

  function interruptAnalysis(): void {
    void deps.aiApi.interrupt_by_user(sessionId);
  }

  function onGuideEvent(e: GuideEvent): void {
    switch (e.kind) {
      case "reasoning":
      case "content":
      case "interrupt":
        appendGuideEvent(e, true);
        if (e.kind === "interrupt") setAnalysisRunning(false);
        return;
      case "sse_done":
        appendGuideEvent(e, true); // collect is a no-op for sse_done
        setAnalysisRunning(false);
        history.push({ format: currentFormat, events: [...activeEvents] });
        renderHistory();
        return;
    }
  }

  function onProviderError(e: ProviderError): void {
    // Pre-flight failure (before any stream): alert and stop the analysis
    // (issue #97). The Game is unaffected.
    setAnalysisRunning(false);
    window.alert(`分析失败：${e.message}`);
  }

  analysisBtn.addEventListener("click", () => {
    if (analysisRunning) interruptAnalysis();
    else void startAnalysis();
  });

  const dispose = (): void => {
    axis.destroy();
    gameArea.dispose();
    container.remove();
  };

  return { dispose };
}
