// Playwright helper for `repro` — reproduce a Minesweeper scene with real
// mouse events and screenshot it.
//
// This is a library, not a CLI: import `openshot` / `Shot` and write a scene
// script. The backend owns the game state (./src, Rust/axum); the only thing
// this file does over the wire is read `/state` and POST `/action`. Every
// gesture is a real mouse event injected in the browser by Playwright.
//
// The service is used if one is already serving the game (a legal `/state`
// response), otherwise the script starts `cargo run` itself. Pass `SHOT_BASE_URL`
// to point at a Vite dev server (which proxies `/state`/`/action` to the
// backend) instead of the backend's own port.
//
// Environment:
//   SHOT_BASE_URL  base URL of a served frontend (default http://127.0.0.1:8080)
//   SHOT_DIR       directory for screenshots (default <repo>/.scratch/shots)

import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const ROOT = fileURLToPath(new URL("../../", import.meta.url)); // repo root
const DEFAULT_BASE = `http://127.0.0.1:8080`;
const DEFAULT_SHOTS = path.join(ROOT, ".scratch", "shots");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** True when `base` serves a live game (legally responds to `/state`). */
async function isGameService(base) {
  try {
    const res = await fetch(`${base}/state`, {
      signal: AbortSignal.timeout(1500),
    });
    if (!res.ok) return false;
    const json = await res.json();
    return typeof json?.game_state === "string" && json?.cells?.length > 0;
  } catch {
    return false;
  }
}

/** Ensures a game is reachable at `base`: reuses one that is already serving,
 * or starts `cargo run` itself. Throws an actionable error when the port is
 * taken by something that is not this game. */
async function ensureServer({ base, seed, prank, onInfo }) {
  if (await isGameService(base)) return { reused: true, proc: null };

  const port = Number(new URL(base).port) || 8080;
  const args = ["run", "--", "--port", String(port)];
  if (seed != null) args.push("--seed", String(seed));
  if (prank) args.push("--prank");
  onInfo?.(`Starting backend: cargo ${args.join(" ")}`);

  const proc = spawn("cargo", args, {
    cwd: ROOT,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let procErr = "";
  proc.stderr.on("data", (d) => (procErr += d.toString()));

  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (await isGameService(base)) return { reused: false, proc };
    if (proc.exitCode !== null) {
      const last = procErr.split("\n").filter(Boolean).slice(-1)[0] || procErr;
      throw new Error(
        `"cargo run" exited (code ${proc.exitCode}) before serving on ${base}. ` +
          `If port ${port} is already taken by another process (e.g. your own ` +
          `"cargo run" running behind you), stop it or use a different port. ` +
          `Last output: ${last}`,
      );
    }
    await sleep(400);
  }
  proc.kill();
  throw new Error(`Backend did not become ready on ${base} within 90s.`);
}

export class Shot {
  constructor({
    base = process.env.SHOT_BASE_URL || DEFAULT_BASE,
    seed,
    prank,
    headless = true,
    onInfo,
  } = {}) {
    this.base = base;
    this.seed = seed;
    this.prank = prank;
    this.headless = headless;
    this.onInfo = onInfo || ((m) => console.log(`[shot] ${m}`));
    this.browser = null;
    this.page = null;
    this.serverProc = null;
    this.reused = false;
    this.shotsDir = process.env.SHOT_DIR || DEFAULT_SHOTS;
  }

  /** Ensures a game service, launches a headless browser, loads the board. */
  async open() {
    const { reused, proc } = await ensureServer({
      base: this.base,
      seed: this.seed,
      prank: this.prank,
      onInfo: this.onInfo,
    });
    this.serverProc = proc;
    this.reused = reused;
    this.onInfo(
      reused
        ? `Reusing game service at ${this.base}`
        : `Started game service at ${this.base}`,
    );
    if (reused && this.seed != null) {
      this.onInfo(
        `Note: --seed is ignored when reusing a service; the running game's ` +
          `board layout applies. To pin a seed, stop the service and let the ` +
          `script start it (or start it yourself with --seed ${this.seed}).`,
      );
    }

    this.browser = await chromium.launch({ headless: this.headless });
    this.page = await this.browser.newPage();
    // `networkidle` can hang: the frontend polls `/state` every second.
    await this.page.goto(this.base, { waitUntil: "domcontentloaded" });
    await this.waitForReady();
    return this;
  }

  /** Current game state (GET /state). */
  async snapshot() {
    const res = await fetch(`${this.base}/state`);
    if (!res.ok) throw new Error(`GET /state failed: ${res.status}`);
    return await res.json();
  }

  /** Waits until the board renders the full grid for the current difficulty. */
  async waitForReady() {
    await this.page.waitForSelector("#board .cell", { state: "attached" });
    const s = await this.snapshot();
    const n = s.rows * s.cols;
    await this.page.waitForFunction(
      (count) => document.querySelectorAll("#board .cell").length === count,
      n,
    );
  }

  /** Center (viewport coords) of the Cell at (row, col). */
  async cellCenter(row, col) {
    const el = this.page.locator(
      `#board .cell[data-row="${row}"][data-col="${col}"]`,
    );
    const box = await el.boundingBox();
    if (!box) throw new Error(`Cell (${row},${col}) not rendered`);
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  }

  /** Reveals a Cell: left press + release on it. */
  async reveal(row, col) {
    const { x, y } = await this.cellCenter(row, col);
    await this.page.mouse.move(x, y);
    await this.page.mouse.down();
    await this.page.mouse.up();
  }

  /** Flags a Cell: right press on it (the flag fires on right-down). */
  async flag(row, col) {
    const { x, y } = await this.cellCenter(row, col);
    await this.page.mouse.move(x, y);
    await this.page.mouse.down({ button: "right" });
    await this.page.mouse.up({ button: "right" });
  }

  /** Moves the pointer to a Cell without pressing anything (drives the
   * Press/Chord Preview follow). */
  async hover(row, col) {
    const { x, y } = await this.cellCenter(row, col);
    await this.page.mouse.move(x, y);
  }

  /**
   * Starts a chainable Chord gesture. Both presses must land on a Revealed
   * Cell to arm; releasing Left solves the Cell under the pointer (the Chord
   * Preview position). The stage methods are synchronous (so they chain) and
   * just queue the mouse steps; `release()` runs them in order then lifts
   * both buttons. Example:
   *   await shot.chord().pressAt(r, c).moveTo(r2, c2).release();
   */
  chord() {
    const shot = this;
    const steps = [];
    const api = {
      pressAt(row, col) {
        steps.push(async () => {
          const { x, y } = await shot.cellCenter(row, col);
          await shot.page.mouse.move(x, y);
          await shot.page.mouse.down({ button: "right" });
          await shot.page.mouse.down({ button: "left" });
        });
        return api;
      },
      moveTo(row, col) {
        steps.push(async () => {
          await shot.hover(row, col);
        });
        return api;
      },
      async release() {
        for (const step of steps) await step();
        await shot.page.mouse.up({ button: "left" });
        await shot.page.mouse.up({ button: "right" });
      },
    };
    return api;
  }

  /** Starts a new game (optionally on a difficulty) via POST /action. */
  async newGame(difficulty) {
    const action = difficulty
      ? { type: "new-game", difficulty }
      : { type: "new-game" };
    const res = await fetch(`${this.base}/action`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(action),
    });
    if (!res.ok) throw new Error(`POST /action failed: ${res.status}`);
    return await res.json();
  }

  /** Screenshots the page into <shotsDir>/<name> and returns the full path. */
  async screenshot(name = "shot.png") {
    if (!path.isAbsolute(name)) {
      fs.mkdirSync(this.shotsDir, { recursive: true });
      name = path.join(this.shotsDir, name);
    }
    await this.page.screenshot({ path: name });
    return name;
  }

  /** Closes the browser; kills the backend only if this script started it. */
  async close() {
    await this.browser?.close();
    this.browser = null;
    if (this.serverProc && this.serverProc.exitCode === null) {
      this.onInfo("Stopping the backend this script started.");
      this.serverProc.kill();
    }
    this.serverProc = null;
  }
}

/** Opens a shot session on a (reused or self-started) game service. */
export async function openshot(opts) {
  const s = new Shot(opts);
  await s.open();
  return s;
}
