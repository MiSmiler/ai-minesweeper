import { postAction } from "../api";
import { bootstrapGamePage } from "../bootstrap";
import { navigateTo, wirePageNav } from "../nav";
import "../style.css";

// The ai-player page: the same interactive Board (reused via the shared
// bootstrap) plus an AI panel that a later step wires to the DeepSeek session.
async function main(): Promise<void> {
  const root = document.getElementById("app");
  if (!root) return;
  wirePageNav(document.body, { post: postAction, navigate: navigateTo });
  try {
    await bootstrapGamePage(root);
  } catch {
    // A load failure is already rendered on the Board by the bootstrap.
  }
}

void main();
