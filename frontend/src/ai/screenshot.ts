// Board screenshot capture for the image presentation form (issue #114).
//
// The real capture uses `html-to-image` (`toPng`) to screenshot `boardEl`
// into a PNG data URL for form D (image). This ticket is the shell only:
// the image data is collected into `GuideRequest.imageDataUrl` by the `app/`
// composition, and the analyzer is a stub. The actual `html-to-image` capture
// is deferred; `captureBoardImage` here returns a valid (empty) PNG data URL
// so the stub flow can exercise the contract. It is injected through
// `AppDeps.captureBoardImage` precisely so jsdom tests can substitute it (the
// browser-only capture never runs under jsdom).

const PLACEHOLDER_PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

/** Screenshots `boardEl` into a PNG data URL (`data:image/png;base64,`).
 * Stub for the shell ticket: returns a valid placeholder. */
export async function captureBoardImage(
  _boardEl: HTMLElement,
  _opts?: { pixelRatio?: number },
): Promise<string> {
  // TODO(#118): use html-to-image's `toPng` (pixelRatio honored) to capture
  // the real board.
  return PLACEHOLDER_PNG;
}
