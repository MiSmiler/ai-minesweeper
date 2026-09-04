// Board axis overlay seam (issue #114; the axis itself is issue #118).
//
// The dashboard's row/col axis checkbox needs to toggle the labels that ring
// the board. #114 only delivers the structural shell: an absolutely-positioned,
// `pointer-events:none` layer wrapped around `boardEl` but *outside* `.board`,
// so it never intercepts a click and never appears in a screenshot of the
// board. The actual 0-based row/col label rendering — `createBoardAxis(boardEl,
// {visible})` + `setRowsCols`/`setVisible`/`destroy` — is #118's product; here
// the API is stubbed to the contract shape so #118 fills in `setRowsCols`.

export interface AxisOverlay {
  /** Renders/refreshes the row/col labels for the given grid. Implemented by
   * #118; the shell's `setRowsCols` is a no-op. */
  setRowsCols(rows: number, cols: number): void;
  /** Shows or hides the axis label overlay (default off). */
  setVisible(visible: boolean): void;
  /** Tears the overlay down (mode switch resets the guide state). */
  destroy(): void;
}

/** Wraps `boardEl` in a `.board-axis-zone` (position:relative) and lays a
 * `.axis-label-layer` (absolute, `pointer-events:none`) over it — the shell
 * the checkbox toggles. The layer stays outside `boardEl`, so a screenshot of
 * `boardEl` never includes the axis. */
export function createBoardAxis(
  boardEl: HTMLElement,
  opts: { visible?: boolean } = {},
): AxisOverlay {
  let zone = boardEl.closest<HTMLElement>(".board-axis-zone");
  if (!zone) {
    zone = document.createElement("div");
    zone.className = "board-axis-zone";
    const parent = boardEl.parentNode;
    parent?.insertBefore(zone, boardEl);
    zone.appendChild(boardEl);
  }

  const labelLayer = document.createElement("div");
  labelLayer.className = "axis-label-layer";
  zone.appendChild(labelLayer);

  // Default off (user story #16); the guide-mode checkbox drives setVisible.
  if (opts.visible ?? false) {
    labelLayer.classList.remove("hidden");
  } else {
    labelLayer.classList.add("hidden");
  }

  const setVisible = (visible: boolean): void => {
    labelLayer.classList.toggle("hidden", !visible);
  };

  const setRowsCols = (_rows: number, _cols: number): void => {
    // Row/col label rendering lands with #118; the shell only owns the overlay
    // structure and the visibility toggle.
  };

  const destroy = (): void => {
    zone.remove();
  };

  return { setRowsCols, setVisible, destroy };
}
