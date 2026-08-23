import { analyzeBoard, postAction } from "../api";
import { bootstrapGamePage } from "../bootstrap";
import { navigateTo, wirePageNav } from "../nav";
import { renderPanelError, renderTranscript } from "./panel";
import "../style.css";

/** Runs a DeepSeek analysis and renders the transcript; a failure shows the
 * error in the panel. The button stays disabled for the round trip so
 * overlapping runs can't interleave. */
function runAnalysis(): void {
  const body = document.getElementById("ai-panel-body");
  const btn = document.getElementById("ai-analyze") as HTMLButtonElement | null;
  if (!body || !btn) return;
  btn.disabled = true;
  void (async () => {
    try {
      const result = await analyzeBoard();
      renderTranscript(body, result);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      renderPanelError(body, message);
    } finally {
      btn.disabled = false;
    }
  })();
}

// The ai-player page: the same interactive Board (reused via the shared
// bootstrap) plus an AI panel that runs a DeepSeek analysis of the board.
async function main(): Promise<void> {
  const root = document.getElementById("app");
  if (!root) return;
  wirePageNav(document.body, { post: postAction, navigate: navigateTo });
  document.getElementById("ai-analyze")?.addEventListener("click", runAnalysis);
  try {
    await bootstrapGamePage(root);
  } catch {
    // A load failure is already rendered on the Board by the bootstrap.
  }
}

void main();
