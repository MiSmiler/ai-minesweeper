// Board screenshot capture for the image presentation form (issue #120).
//
// `captureBoardImage` screenshots `boardEl` into a PNG data URL for form D
// (image) using `html-to-image`'s `toPng`. It honors the caller's `pixelRatio`
// (the image-form analyze flow passes `1` so the board is not enlarged, keeping
// the image token budget low); the default is also `1` (no upscaling). The
// browser-only capture never runs under jsdom — the compositions receive it via
// `AppDeps.captureBoardImage`, so jsdom tests substitute a stub.

import { toPng } from "html-to-image";

/** Screenshots `boardEl` into a PNG data URL (`data:image/png;base64,`).
 * `pixelRatio` defaults to `1` so the returned image is not enlarged. */
export async function captureBoardImage(
  boardEl: HTMLElement,
  opts?: { pixelRatio?: number },
): Promise<string> {
  return toPng(boardEl, { pixelRatio: opts?.pixelRatio ?? 1 });
}
