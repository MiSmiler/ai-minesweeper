// The frontend entry (ADR-0023): builds the injected dependencies and mounts
// the single page layout. There is no PlayMode and no mode switcher — one Game
// is shown one way, with the AiPlayer driven by hand from the dashboard.

import { createAgentApi } from "../agent/api";
import { createAiPlayerApi } from "../ai-player/api";
import { captureBoardImage } from "../ai-player/screenshot";
import { mountLayout, type AppDeps } from "./layout";
import "../style.css";

const ROOT = document.getElementById("app")!;
const deps: AppDeps = {
  // The binding's half: `/ai/begin` and `/ai/send` (the board's Send, whose
  // SSE stream it consumes through the Agent's reader).
  aiPlayerApi: createAiPlayerApi(),
  // The Agent's half: `/ai/messages` and `/ai/interrupt`.
  agentApi: createAgentApi(),
  captureBoardImage,
};
const layout = mountLayout(ROOT, deps);

// A refresh (or tab close) would silently discard a live AI Session; the
// browser shows a native beforeunload prompt when the guard reports one.
// A custom confirm can't block unload, so this is the only browser-sanctioned
// way to warn (issue #112 US-32 spirit).
window.addEventListener("beforeunload", (e) => {
  if (layout.hasUsedSession()) e.preventDefault();
});
