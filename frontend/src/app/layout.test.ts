// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountLayout } from "./layout";
import type { AppDeps } from "./layout";
import { makeGameSnapshot } from "../game/testUtils";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/** Stubs the global fetch so the game area's client loads against a snapshot. */
function mockFetch(snapshot = makeGameSnapshot()): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async () =>
        ({
          ok: true,
          json: async () => snapshot,
        }) as Response,
    ),
  );
}

/**
 * The composition's dependencies over stubbed apis: the binding's two routes
 * and the Agent's. The Send records the callbacks the composition hands it.
 */
function makeDeps(): AppDeps {
  return {
    aiPlayerApi: {
      begin: vi.fn(async () => {}),
      send: vi.fn(),
    },
    agentApi: {
      messages: vi.fn().mockResolvedValue([]),
      interrupt: vi.fn().mockResolvedValue(undefined),
    },
    captureBoardImage: vi.fn().mockResolvedValue("data:image/png;base64,xxx"),
  };
}

/** The `onEvent` / `onFailure` callbacks the composition hands to the Nth Send
 * (the 2nd and 3rd arguments of `AiPlayerApi.send`). */
const onEvent = (deps: AppDeps, n = 0) =>
  vi.mocked(deps.aiPlayerApi.send).mock.calls[n][1];
const onFailure = (deps: AppDeps, n = 0) =>
  vi.mocked(deps.aiPlayerApi.send).mock.calls[n][2];

function mount(): HTMLElement {
  const root = document.createElement("div");
  document.body.appendChild(root);
  return root;
}

function $(root: HTMLElement, sel: string): HTMLElement {
  return root.querySelector(sel)!;
}

/** Clicks the session button's start face and waits for the backend round
 * trip. The button only starts a Session while none is live. */
async function startSession(root: HTMLElement): Promise<void> {
  root.querySelector<HTMLButtonElement>(".session-btn")!.click();
  await flush();
}

/** Clicks the session button's close face and waits for the interruption. */
async function closeSession(root: HTMLElement): Promise<void> {
  root.querySelector<HTMLButtonElement>(".session-btn")!.click();
  await flush();
}

/** Creates a session and sends one board so the session is used. */
async function seedUsedSession(
  root: HTMLElement,
  deps: AppDeps,
): Promise<void> {
  await startSession(root);
  root.querySelector<HTMLButtonElement>(".send-btn")!.click();
  onEvent(deps)({ kind: "content", text: "(2,3)" });
  onEvent(deps)({ kind: "sse_done" });
  await flush();
}

beforeEach(() => {
  document.body.innerHTML = "";
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("mountLayout layout", () => {
  it("builds the board column and the AI column", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const layout = root.querySelector(".layout")!;
    expect(layout.querySelector(".game-column")).toBeTruthy();
    expect(layout.querySelector(".ai-column")).toBeTruthy();
    // The board column holds a fully independent game body.
    expect(layout.querySelector(".game-column .game-body")).toBeTruthy();
    expect(layout.querySelector(".game-column .status-bar")).toBeTruthy();
  });

  it("stacks the dashboard above the SessionBox in the AI column", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const column = root.querySelector(".ai-column")!;
    expect(column.children[0]?.classList.contains("ai-dashboard")).toBe(true);
    expect(column.children[1]?.classList.contains("ai-session-box")).toBe(true);
  });

  it("dashboard has the session button, input mode, level — no strategy", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const dash = $(root, ".ai-dashboard");
    expect(dash.querySelector(".session-btn")).toBeTruthy();
    expect(dash.querySelector(".input-mode-select")).toBeTruthy();
    expect(dash.querySelector(".level-select")).toBeTruthy();
    // Send and the axis toggle moved to the aux bar (below the Board).
    expect(dash.querySelector(".send-btn")).toBeNull();
    expect(dash.querySelector(".axis-checkbox")).toBeNull();
    // The SessionStrategy dropdown is gone (issue #125).
    expect(dash.querySelector(".strategy-select")).toBeNull();
    expect(dash.textContent).not.toContain("会话策略");
  });

  it("puts the axis toggle and Send in the aux bar below the Board", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const bar = $(root, ".game-column .aux-bar");
    expect(bar.querySelector(".axis-checkbox")).toBeTruthy();
    expect(bar.querySelector(".send-btn")).toBeTruthy();
    // Axis toggle left, Send right.
    expect(bar.children[0]?.classList.contains("axis-toggle")).toBe(true);
    expect(bar.children[1]?.classList.contains("send-btn")).toBe(true);
  });

  it("input mode select offers all three modes", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const opts = $(root, ".input-mode-select").querySelectorAll("option");
    expect(opts).toHaveLength(3);
    expect(Array.from(opts).map((o) => o.value)).toEqual([
      "plain",
      "emoji",
      "image",
    ]);
  });

  it("level select offers the four levels, default low", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const select = $(root, ".level-select") as HTMLSelectElement;
    const opts = select.querySelectorAll("option");
    expect(opts).toHaveLength(4);
    expect(Array.from(opts).map((o) => o.value)).toEqual([
      "off",
      "low",
      "high",
      "max",
    ]);
    expect(select.value).toBe("low");
  });

  it("the axis layer is hidden by default", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const layer = root.querySelector(".axis-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("renders 0-based row/col labels for the loaded board (issue #118)", async () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    await flush(); // let the board load, which fires onRender → setRowsCols
    const layer = root.querySelector(".axis-layer")!;
    const rows = layer.querySelectorAll(".axis-row");
    const cols = layer.querySelectorAll(".axis-col");
    expect(rows).toHaveLength(2); // makeGameSnapshot() is 2×2
    expect(cols).toHaveLength(2);
    expect(Array.from(rows).map((e) => e.textContent)).toEqual(["0", "1"]);
    expect(Array.from(cols).map((e) => e.textContent)).toEqual(["0", "1"]);
  });
});

describe("mountLayout session controls", () => {
  it("disables Send with no session and leaves the InputMode select enabled", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;
    expect(send.disabled).toBe(true);
    expect(mode.disabled).toBe(false);
  });

  it("shows the Session's system prompt as soon as it begins", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    vi.mocked(deps.agentApi.messages).mockResolvedValueOnce([
      { role: "system", content: "be helpful" },
    ]);
    mountLayout(root, deps);

    await startSession(root);

    expect($(root, ".session-system").textContent).toBe("be helpful");
    expect($(root, ".ai-session-box").style.display).toBe("");
  });

  it("a new session enables Send and locks the InputMode select", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;
    expect(send.disabled).toBe(false);
    // The Session's InputMode is fixed from here on.
    expect(mode.disabled).toBe(true);
    expect(deps.aiPlayerApi.begin).toHaveBeenCalledWith("plain");
  });

  it("the live Session locks the InputMode", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;
    expect(mode.disabled).toBe(true);

    send.click(); // running: still locked
    expect(mode.disabled).toBe(true);
    expect(send.textContent).toBe("中断");
    onEvent(deps)({ kind: "sse_done" });
    expect(mode.disabled).toBe(true); // and after the reply
    expect(send.textContent).toBe("发送");
  });

  it("an interrupted Send keeps the InputMode locked", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    const mode = $(root, ".input-mode-select") as HTMLSelectElement;

    send.click();
    onEvent(deps)({ kind: "interrupted" });
    expect(mode.disabled).toBe(true);
    expect($(root, ".session-interrupt").textContent).toContain("已中断");
  });

  it("Send is a no-op with no session", () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    ($(root, ".send-btn") as HTMLButtonElement).click();
    expect(deps.aiPlayerApi.send).not.toHaveBeenCalled();
  });
});

describe("mountLayout send flow", () => {
  it("streams events into the box", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;

    send.click();
    expect(deps.aiPlayerApi.send).toHaveBeenCalledTimes(1);
    // The composition root builds the request: the level, and no screenshot
    // outside the image mode.
    expect(vi.mocked(deps.aiPlayerApi.send).mock.calls[0][0]).toEqual({
      thinkingLevel: "low",
      imageDataUrl: undefined,
    });

    onEvent(deps)({ kind: "reasoning", text: "think" });
    onEvent(deps)({ kind: "content", text: "(2,3)" });
    expect($(root, ".session-reasoning").textContent).toBe("think");
    expect($(root, ".session-content").textContent).toBe("(2,3)");

    onEvent(deps)({ kind: "sse_done" });
  });

  it("interrupt calls the Agent and the event reverts the button", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    send.click(); // send → interrupt
    expect(deps.agentApi.interrupt).toHaveBeenCalledTimes(1);

    onEvent(deps)({ kind: "interrupted" });
    expect(send.textContent).toBe("发送");
  });

  it("captures a screenshot for the image mode", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    // The mode is picked before the Session exists: `begin` fixes it.
    const select = root.querySelector<HTMLSelectElement>(".input-mode-select")!;
    select.value = "image";
    select.dispatchEvent(new Event("change"));
    await startSession(root);

    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    await flush();
    expect(deps.captureBoardImage).toHaveBeenCalled();
    expect(deps.aiPlayerApi.begin).toHaveBeenCalledWith("image");
    const req = vi.mocked(deps.aiPlayerApi.send).mock.calls[0][0] as {
      imageDataUrl?: string;
    };
    expect(req.imageDataUrl).toBeTruthy();
  });

  it("sends the selected thinking level across a change", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;

    send.click();
    let req = vi.mocked(deps.aiPlayerApi.send).mock.calls[0][0] as {
      thinkingLevel: string;
    };
    expect(req.thinkingLevel).toBe("low");
    onEvent(deps)({ kind: "sse_done" });

    // Switch the level: no confirm.
    const level = root.querySelector<HTMLSelectElement>(".level-select")!;
    level.value = "max";
    level.dispatchEvent(new Event("change"));

    // The next Send carries the new level.
    send.click();
    req = vi.mocked(deps.aiPlayerApi.send).mock.calls[1][0] as {
      thinkingLevel: string;
    };
    expect(req.thinkingLevel).toBe("max");
  });

  it("a provider failure alerts and reverts the button", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    onFailure(deps)({
      kind: "provider",
      error: { kind: "config", code: null, message: "no provider" },
    });
    expect(alertSpy).toHaveBeenCalled();
    expect(send.textContent).toBe("发送");
  });

  it("a refused failure does not alert", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    mountLayout(root, deps);
    await startSession(root);
    const send = $(root, ".send-btn") as HTMLButtonElement;
    send.click();
    onFailure(deps)({ kind: "refused", status: 409, message: "busy" });
    expect(alertSpy).not.toHaveBeenCalled();
    expect(send.textContent).toBe("发送");
  });

  it("streams the user message into the box", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click();
    onEvent(deps)({ kind: "user", text: "........." });
    expect($(root, ".session-user-text").textContent).toBe(".........");
  });

  it("shows an image thumbnail in the box for the image form", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    const select = root.querySelector<HTMLSelectElement>(".input-mode-select")!;
    select.value = "image";
    select.dispatchEvent(new Event("change"));
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click();
    await flush(); // let captureBoardImage resolve; send seeds userImageUrl
    const img = root.querySelector(".session-user-image") as HTMLImageElement;
    expect(img.hidden).toBe(false);
    expect(img.src).toBe("data:image/png;base64,xxx");
  });
});

describe("mountLayout session lifecycle", () => {
  it("the session button's text follows the session", async () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    const session = $(root, ".session-btn") as HTMLButtonElement;
    expect(session.textContent).toBe("启动AI会话");

    await startSession(root);
    expect(session.textContent).toBe("关闭AI会话");

    await closeSession(root);
    expect(session.textContent).toBe("启动AI会话");
  });

  it("the session button is disabled while begin() is in flight", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    let settleBegin: () => void = () => {};
    vi.mocked(deps.aiPlayerApi.begin).mockImplementation(
      () => new Promise<void>((resolve) => (settleBegin = resolve)),
    );
    mountLayout(root, deps);
    const session = $(root, ".session-btn") as HTMLButtonElement;

    session.click();
    expect(session.disabled).toBe(true);
    session.click(); // a second click while the first begin() is pending
    expect(deps.aiPlayerApi.begin).toHaveBeenCalledTimes(1);
    expect(session.textContent).toBe("启动AI会话"); // still `none`

    settleBegin();
    await flush();
    expect(session.disabled).toBe(false);
    expect(session.textContent).toBe("关闭AI会话");
  });
  it("a failed new session alerts and leaves Send disabled", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    vi.mocked(deps.aiPlayerApi.begin).mockRejectedValueOnce({
      kind: "config",
      code: null,
      message: "no provider",
    });
    mountLayout(root, deps);
    await startSession(root);
    expect(alertSpy).toHaveBeenCalled();
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(true);
  });

  it("closing a used session discards it after confirmation", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    mountLayout(root, deps);
    await seedUsedSession(root, deps);

    await closeSession(root);
    expect(confirmSpy).toHaveBeenCalled();
    // The Session is gone: the box hides, Send disables, the lock reopens.
    expect($(root, ".ai-session-box").style.display).toBe("none");
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(true);
    expect(
      (root.querySelector(".input-mode-select") as HTMLSelectElement).disabled,
    ).toBe(false);

    // A closed session is gone for good: starting again is a fresh begin().
    await startSession(root);
    expect(deps.aiPlayerApi.begin).toHaveBeenCalledTimes(2);
  });

  it("declining to close keeps the current session", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    vi.spyOn(window, "confirm").mockReturnValue(false);
    mountLayout(root, deps);
    await seedUsedSession(root, deps);

    await closeSession(root);
    // The used session survives: the box stays and the InputMode locked.
    expect($(root, ".ai-session-box").style.display).toBe("");
    expect(
      (root.querySelector(".input-mode-select") as HTMLSelectElement).disabled,
    ).toBe(true);
  });

  it("closing an unused session never confirms", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    mountLayout(root, deps);
    await startSession(root);
    confirmSpy.mockClear();

    await closeSession(root);
    expect(confirmSpy).not.toHaveBeenCalled();
    expect($(root, ".ai-session-box").style.display).toBe("none");
  });

  it("closing interrupts an in-flight Send first", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    // A Send makes the Session `used`, so closing it asks first.
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mountLayout(root, deps);
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click(); // running

    await closeSession(root);
    expect(deps.agentApi.interrupt).toHaveBeenCalledTimes(1);
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(true);
  });

  it("a new game ends the session after confirmation", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    mountLayout(root, deps);
    await flush(); // init done, so the game area click listener is live
    await seedUsedSession(root, deps);

    const difficulty = root.querySelector<HTMLButtonElement>(
      '[data-difficulty="beginner"]',
    )!;
    difficulty.click();
    await flush();
    expect(confirmSpy).toHaveBeenCalled();
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(true);
  });

  it("declining a new game keeps the game and the session", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    mountLayout(root, deps);
    await flush();
    await seedUsedSession(root, deps);

    const difficulty = root.querySelector<HTMLButtonElement>(
      '[data-difficulty="beginner"]',
    )!;
    difficulty.click();
    await flush();
    expect(confirmSpy).toHaveBeenCalled();
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(false);
  });

  it("the smiley starts a new game after confirmation", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mountLayout(root, deps);
    await flush();
    await seedUsedSession(root, deps);

    const smiley = root.querySelector<HTMLButtonElement>(".smiley")!;
    smiley.click();
    await flush();
    expect(($(root, ".send-btn") as HTMLButtonElement).disabled).toBe(true);
  });

  it("a new game on an in-flight Send confirms and interrupts", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    mountLayout(root, deps);
    await flush();
    await startSession(root);
    ($(root, ".send-btn") as HTMLButtonElement).click(); // running, already used

    const smiley = root.querySelector<HTMLButtonElement>(".smiley")!;
    smiley.click();
    await flush();
    // A Send makes the Session `used` even before its reply lands.
    expect(confirmSpy).toHaveBeenCalled();
    expect(deps.agentApi.interrupt).toHaveBeenCalledTimes(1);
  });

  it("reports hasUsedSession for the refresh guard", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const layout = mountLayout(root, deps);
    expect(layout.hasUsedSession()).toBe(false);

    // A created but unused session does not guard either.
    await startSession(root);
    expect(layout.hasUsedSession()).toBe(false);

    // A Send makes it used before any reply lands.
    ($(root, ".send-btn") as HTMLButtonElement).click();
    expect(layout.hasUsedSession()).toBe(true);
    onEvent(deps)({ kind: "sse_done" });
    await flush();
    expect(layout.hasUsedSession()).toBe(true);

    // Closing drops what the guard was protecting.
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await closeSession(root);
    expect(layout.hasUsedSession()).toBe(false);
  });

  it("the axis checkbox toggles the axis layer", () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    mountLayout(root, deps);
    const checkbox = root.querySelector<HTMLInputElement>(".axis-checkbox")!;
    const layer = root.querySelector(".axis-layer")!;
    expect(layer.classList.contains("hidden")).toBe(true);
    checkbox.checked = true;
    checkbox.dispatchEvent(new Event("change"));
    expect(layer.classList.contains("hidden")).toBe(false);
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event("change"));
    expect(layer.classList.contains("hidden")).toBe(true);
  });

  it("dispose tears down the layout", () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    const { dispose } = mountLayout(root, deps);
    dispose();
    expect(root.querySelector(".layout")).toBeNull();
  });
});

describe("mountLayout SessionBox visibility", () => {
  it("is hidden with no session", () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    expect($(root, ".ai-session-box").style.display).toBe("none");
  });

  it("appears once the session starts", async () => {
    mockFetch();
    const root = mount();
    mountLayout(root, makeDeps());
    await startSession(root);
    expect($(root, ".ai-session-box").style.display).toBe("");
  });

  it("stays hidden when the session start fails", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    vi.spyOn(window, "alert").mockImplementation(() => {});
    vi.mocked(deps.aiPlayerApi.begin).mockRejectedValueOnce({
      kind: "config",
      code: null,
      message: "no provider",
    });
    mountLayout(root, deps);
    await startSession(root);
    expect($(root, ".ai-session-box").style.display).toBe("none");
  });

  it("hides again when a new game ends the session", async () => {
    mockFetch();
    const root = mount();
    const deps = makeDeps();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mountLayout(root, deps);
    await flush(); // init done, so the game area click listener is live
    await seedUsedSession(root, deps);
    expect($(root, ".ai-session-box").style.display).toBe("");

    root
      .querySelector<HTMLButtonElement>('[data-difficulty="beginner"]')!
      .click();
    await flush();
    expect($(root, ".ai-session-box").style.display).toBe("none");
  });
});
