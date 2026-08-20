import type { Pos } from "./api";
import type { ChordPreview } from "./gesture";

/** The preview highlight layer: renders the gesture machine's Previews onto
 * the Board and supports retaining the highlight through an action's round
 * trip (see retain/release). Pure DOM — the machine stays pure. */
export interface PreviewLayer {
  /** Renders the machine's current Previews: clears the highlight, shows
   * the Press Preview Cell and the Chord Preview Cells, then re-shows any
   * retained highlight (an action in flight). */
  render(chordPreview: ChordPreview | null, pressPreview: Pos | null): void;
  /** Retains the currently shown highlight so render() keeps showing it
   * until release(). Call when a Reveal/Chord action is sent, so the Cells
   * do not flash back to Hidden while the response is in flight. */
  retain(): void;
  /** Drops the retained highlight and removes it from the Board. Call when
   * the action's response re-renders (a no-op: the fresh Board has no
   * retained classes) or when the request fails (restores the true state). */
  release(): void;
}

/** A preview highlight layer over `board`. The layer owns the `.cell-preview`
 * class on the Board's Cells: render() shows the machine's current Previews,
 * retain() keeps the last shown highlight alive across renders — the
 * Reveal/Chord round trip — and release() drops it.
 *
 * Known limitation: when two full press–release cycles complete within one
 * request round trip (sub-round-trip double-clicking), the older action's
 * response can drop the newer action's retained highlight early, flashing its
 * Cells until the newer response renders. Self-correcting; accepted. */
export function createPreviewLayer(board: HTMLElement): PreviewLayer {
  let currentChord: ChordPreview | null = null;
  let currentPress: Pos | null = null;
  let retainedChord: ChordPreview | null = null;
  let retainedPress: Pos | null = null;

  const cell = (pos: Pos): HTMLElement | null =>
    board.querySelector(`[data-row="${pos.row}"][data-col="${pos.col}"]`);

  const add = (pos: Pos): void => {
    cell(pos)?.classList.add("cell-preview");
  };

  const remove = (pos: Pos): void => {
    cell(pos)?.classList.remove("cell-preview");
  };

  const addAll = (cells: readonly Pos[]): void => {
    for (const p of cells) add(p);
  };

  const removeAll = (cells: readonly Pos[]): void => {
    for (const p of cells) remove(p);
  };

  return {
    render(chordPreview, pressPreview) {
      currentChord = chordPreview;
      currentPress = pressPreview;
      board
        .querySelectorAll(".cell-preview")
        .forEach((el) => el.classList.remove("cell-preview"));
      if (pressPreview) add(pressPreview);
      if (chordPreview) addAll(chordPreview.cells);
      if (retainedPress) add(retainedPress);
      if (retainedChord) addAll(retainedChord.cells);
    },
    retain() {
      retainedChord = currentChord;
      retainedPress = currentPress;
    },
    release() {
      if (retainedPress) remove(retainedPress);
      if (retainedChord) removeAll(retainedChord.cells);
      retainedChord = null;
      retainedPress = null;
    },
  };
}
