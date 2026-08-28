import type { Position } from "../api";
import type { Preview } from "../logic/preview";

/** The preview highlight layer: renders the gesture machine's Preview onto the
 * Board and supports retaining the highlight through an action's round trip
 * (see retain/release). Pure DOM — the machine stays pure. */
export interface PreviewRenderer {
  /** Renders the machine's current Preview: clears the highlight, shows the
   * Preview's Cells, then re-shows any retained highlight (an action in
   * flight). */
  render(preview: Preview | null): void;
  /** Retains the currently shown highlight so render() keeps showing it until
   * release(). Call when a Reveal/Chord action is sent, so the Cells do not
   * flash back to Hidden while the response is in flight. */
  retain(): void;
  /** Drops the retained highlight and removes it from the Board. Call when the
   * action's response re-renders (a no-op: the fresh Board has no retained
   * classes) or when the request fails (restores the true state). */
  release(): void;
}

/** A preview highlight layer over `board`. The layer owns the `.cell-preview`
 * class on the Board's Cells: render() shows the machine's current Preview,
 * retain() keeps the last shown highlight alive across renders — the
 * Reveal/Chord round trip — and release() drops it.
 *
 * Known limitation: when two full press–release cycles complete within one
 * request round trip (sub-round-trip double-clicking), the older action's
 * response can drop the newer action's retained highlight early, flashing its
 * Cells until the newer response renders. Self-correcting; accepted. */
export function createPreviewRenderer(board: HTMLElement): PreviewRenderer {
  let current: Preview | null = null;
  let retained: Preview | null = null;

  const cell = (pos: Position): HTMLElement | null =>
    board.querySelector(`[data-row="${pos.row}"][data-col="${pos.col}"]`);

  const add = (pos: Position): void => {
    cell(pos)?.classList.add("cell-preview");
  };

  const remove = (pos: Position): void => {
    cell(pos)?.classList.remove("cell-preview");
  };

  const addAll = (cells: readonly Position[]): void => {
    for (const p of cells) add(p);
  };

  const removeAll = (cells: readonly Position[]): void => {
    for (const p of cells) remove(p);
  };

  /** Highlights the Cells a Preview names: the single Cell for a Press Preview,
   * or the highlight set for a Chord Preview. A null Preview highlights nothing. */
  const addPreview = (p: Preview | null): void => {
    if (!p) return;
    if (p.kind === "press") add(p.pos);
    else addAll(p.cells);
  };

  return {
    render(preview) {
      current = preview;
      board
        .querySelectorAll(".cell-preview")
        .forEach((el) => el.classList.remove("cell-preview"));
      addPreview(preview);
      addPreview(retained);
    },
    retain() {
      retained = current;
    },
    release() {
      if (retained) {
        if (retained.kind === "press") remove(retained.pos);
        else removeAll(retained.cells);
      }
      retained = null;
    },
  };
}
