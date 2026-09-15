// The Board axis overlay: 0-based row/col labels ringing the Board, so the
// HumanPlayer driving the AiPlayer can line a reply's `(row, col)` up with a
// Cell. It is a temporary driver affordance (ADR-0019: the manual Send goes
// when the tool loop lands), carries no ai-player vocabulary, and lives with
// the composition that mounts it rather than in the `ai-player` slice.
//
// The label layer is a sibling of `boardEl` inside a `.board-axis-zone`
// wrapper, absolutely positioned and `pointer-events: none`: it never
// intercepts a Gesture and never appears in a screenshot of the Board.
//
// Known debt: `measure` reads the game slice's private DOM (`.board`) and
// falls back to the `--cell-size`-derived pitch constant. The geometry should
// come from the game slice (`measureBoard` / `BoardGeometry`).

export interface BoardAxis {
  /** Renders/refreshes the row/col labels for the given grid.
   * Renders `rows` 0-based row labels along the left edge and `cols`
   * 0-based col labels along the top edge. Re-renders on resize; a no-op
   * when the grid size is unchanged. */
  setRowsCols(rows: number, cols: number): void;
  /** Shows or hides the axis label overlay (default off). */
  setVisible(visible: boolean): void;
  /** Tears the overlay down (mode switch resets the AiPlayer state). The
   * wrapper takes `boardEl` with it, so this is safe whether or not the board
   * host is still attached. */
  destroy(): void;
}

/** Wraps `boardEl` in a `.board-axis-zone` (position:relative) and lays a
 * `.axis-label-layer` (absolute, `pointer-events:none`) over it — the shell
 * the checkbox toggles. The layer stays outside `boardEl`, so a screenshot of
 * `boardEl` never includes the axis. */
export function createBoardAxis(
  boardEl: HTMLElement,
  opts: { visible?: boolean } = {},
): BoardAxis {
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

  // Default off (user story #16); the AiPlay mode's checkbox drives setVisible.
  if (opts.visible ?? false) {
    labelLayer.classList.remove("hidden");
  } else {
    labelLayer.classList.add("hidden");
  }

  const setVisible = (visible: boolean): void => {
    labelLayer.classList.toggle("hidden", !visible);
  };

  /** The rendered grid size, tracked so a same-size re-render (a Reveal,
   * a Flag, a timer tick) doesn't churn the label DOM. */
  let lastRows = -1;
  let lastCols = -1;

  /** The rendered grid cell pitch and total Board height, measured from the
   * `.board` rect so the labels track the grid as laid out (gap included).
   * Falls back to the CSS constants when the Board isn't laid out yet (jsdom,
   * or a pre-render Board). */
  const measure = (
    rows: number,
    cols: number,
  ): { pitchX: number; pitchY: number; height: number } => {
    const board = boardEl.querySelector<HTMLElement>(".board");
    const rect = board?.getBoundingClientRect();
    let pitchX = 25.5; // --cell-size (24px) + the 1.5px hairline.
    let pitchY = 25.5;
    let height = 0;
    if (rect && Number.isFinite(rect.width) && rect.width > 0) {
      pitchX = rect.width / cols;
    }
    if (rect && Number.isFinite(rect.height) && rect.height > 0) {
      pitchY = rect.height / rows;
      height = rect.height;
    }
    return { pitchX, pitchY, height };
  };

  const setRowsCols = (rows: number, cols: number): void => {
    if (rows === lastRows && cols === lastCols) return;
    lastRows = rows;
    lastCols = cols;

    labelLayer
      .querySelectorAll(".axis-row, .axis-col")
      .forEach((el) => el.remove());

    const { pitchX, pitchY, height } = measure(rows, cols);

    // Row labels run down the Board's left edge (issue #118: left + bottom
    // axis), right-aligned toward it and centered on each Row's midline.
    for (let r = 0; r < rows; r++) {
      const row = document.createElement("div");
      row.className = "axis-row";
      row.dataset.row = String(r);
      row.textContent = String(r);
      row.style.top = `${r * pitchY + pitchY / 2}px`;
      labelLayer.appendChild(row);
    }

    // Column labels run along the Board's bottom edge, just below it — so they
    // never overlap the top bar or the Cells (issue #118: left + bottom axis).
    for (let c = 0; c < cols; c++) {
      const col = document.createElement("div");
      col.className = "axis-col";
      col.dataset.col = String(c);
      col.textContent = String(c);
      col.style.left = `${c * pitchX + pitchX / 2}px`;
      col.style.top = `${height + 2}px`;
      labelLayer.appendChild(col);
    }
  };

  const destroy = (): void => {
    zone.remove();
  };

  return { setRowsCols, setVisible, destroy };
}
