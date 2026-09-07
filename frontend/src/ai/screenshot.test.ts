// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { captureBoardImage } from "./screenshot";
import { toPng } from "html-to-image";

// jsdom cannot run a real html-to-image capture (no layout/canvas), so the
// browser-only `toPng` is mocked and we assert the forwarding contract.
vi.mock("html-to-image", () => ({
  toPng: vi.fn(),
}));

afterEach(() => {
  vi.mocked(toPng).mockReset();
});

describe("captureBoardImage", () => {
  it("returns the PNG data URL produced by html-to-image, defaulting pixelRatio to 1", async () => {
    const el = document.createElement("div");
    vi.mocked(toPng).mockResolvedValue("data:image/png;base64,AAAA");
    const url = await captureBoardImage(el);
    expect(url.startsWith("data:image/png;base64,")).toBe(true);
    expect(toPng).toHaveBeenCalledWith(el, { pixelRatio: 1 });
  });

  it("honors a caller-supplied pixelRatio", async () => {
    const el = document.createElement("div");
    vi.mocked(toPng).mockResolvedValue("data:image/png;base64,BBBB");
    await captureBoardImage(el, { pixelRatio: 2 });
    expect(toPng).toHaveBeenCalledWith(el, { pixelRatio: 2 });
  });
});
