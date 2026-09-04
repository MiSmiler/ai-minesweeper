// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { captureBoardImage } from "./screenshot";

describe("captureBoardImage", () => {
  it("returns a PNG data URL", async () => {
    const url = await captureBoardImage(document.createElement("div"));
    expect(url.startsWith("data:image/png;base64,")).toBe(true);
  });

  it("honors the pixelRatio option (signature contract)", async () => {
    await expect(
      captureBoardImage(document.createElement("div"), { pixelRatio: 2 }),
    ).resolves.toBeTypeOf("string");
  });
});
