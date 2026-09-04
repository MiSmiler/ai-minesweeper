// The frontend entry: reads the initial PlayMode, renders the top-bar mode
// switcher, and mounts the mode composition. This replaces the old single
// `main.ts` (the single composition now lives in `singleMode.ts`).
//
// Modes are exclusive (ADR-0012): switching abandons the current Game and
// mounts a fresh composition. The initial mode comes from `?mode=` (default
// `single`) so dev/screenshots/acceptance can boot straight into either mode.

import type { AiApi, GuideEvent } from "../ai/api";
import { captureBoardImage } from "../ai/screenshot";
import {
  mountMode,
  renderModeSwitcher,
  type AppDeps,
  type PlayModeName,
} from "./mode";
import "../style.css";

const OPTS: ReadonlyArray<[PlayModeName, string]> = [
  ["single", "single"],
  ["ai-guide", "ai-guide"],
];

/** Reads the initial PlayMode from `?mode=` (validated), defaulting to
 * `single`. */
function readInitialMode(): PlayModeName {
  const mode = new URLSearchParams(window.location.search).get("mode");
  return OPTS.some(([name]) => name === mode)
    ? (mode as PlayModeName)
    : "single";
}

/** A stub `AiApi` for this shell ticket: it emits a short placeholder analysis
 * (`reasoning` → `content` → `sse_done`) so the analyze/interrupt button and the
 * dialog can be exercised without the backend. The real SSE consumer is a
 * later ticket. */
function makeStubAiApi(): AiApi {
  return {
    startGuide(_sessionId, _req, onEvent: (e: GuideEvent) => void) {
      window.setTimeout(() => {
        onEvent({ kind: "reasoning", text: "（stub）正在分析棋盘…" });
      }, 60);
      window.setTimeout(() => {
        onEvent({
          kind: "content",
          text: "分析功能将在后续 ticket 接入真实后端。（stub）",
        });
      }, 120);
      window.setTimeout(() => {
        onEvent({ kind: "sse_done" });
      }, 180);
    },
    interrupt_by_user(_sessionId) {
      return Promise.resolve();
    },
  };
}

const app = document.getElementById("app")!;
const deps: AppDeps = {
  getPlayMode: readInitialMode,
  aiApi: makeStubAiApi(),
  captureBoardImage,
};

// The shell: a mode-switcher bar above a mode-content region.
const switcher = document.createElement("div");
switcher.id = "mode-switcher";
const content = document.createElement("div");
content.id = "mode-content";
app.append(switcher, content);

let current = deps.getPlayMode();
let disposeCurrent = mountMode(current, content, deps);

function refreshSwitcher(): void {
  renderModeSwitcher(switcher, current, (next) => {
    if (next === current) return;
    disposeCurrent();
    current = next;
    disposeCurrent = mountMode(current, content, deps);
    refreshSwitcher();
  });
}
refreshSwitcher();
