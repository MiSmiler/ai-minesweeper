// The frontend entry: reads the initial PlayMode, renders the top-bar mode
// switcher, and mounts the mode composition. This replaces the old single
// `main.ts` (the single composition now lives in `singleMode.ts`).
//
// Modes are exclusive (ADR-0012): switching abandons the current Game and
// mounts a fresh composition. The initial mode comes from `?mode=` (default
// `single`) so dev/screenshots/acceptance can boot straight into either mode.

import { createAiApi } from "../ai/api";
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

/** Persists the chosen PlayMode into `?mode=` so a refresh keeps the current
 * mode. `replaceState` avoids a reload and doesn't add history entries
 * (ADR-0012: modes are exclusive, switching abandons the Game). */
function persistMode(mode: PlayModeName): void {
  const params = new URLSearchParams(window.location.search);
  params.set("mode", mode);
  history.replaceState(null, "", `?${params.toString()}`);
}

const app = document.getElementById("app")!;
const deps: AppDeps = {
  getPlayMode: readInitialMode,
  // The real AI guide transport: consumes the backend `/ai/guide` SSE stream
  // (issue #119).
  aiApi: createAiApi(),
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
    persistMode(current);
    disposeCurrent = mountMode(current, content, deps);
    refreshSwitcher();
  });
}
refreshSwitcher();
