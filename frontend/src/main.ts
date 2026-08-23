import { postAction } from "./api";
import { bootstrapGamePage } from "./bootstrap";
import { log } from "./log";
import { navigateTo, wirePageNav } from "./nav";
import "./style.css";

// The human-player page. The Board input, top-bar controls, and rendering are
// handled by the shared game-page bootstrap, which the ai page also reuses.
async function main(): Promise<void> {
  const root = document.getElementById("app");
  if (!root) {
    log.error("Failed to load game: missing #app");
    return;
  }
  wirePageNav(document.body, { post: postAction, navigate: navigateTo });
  try {
    await bootstrapGamePage(root);
  } catch (err) {
    const message = err instanceof Error ? err.message : err;
    log.error(`Failed to load game: ${message}`);
  }
}

void main();
