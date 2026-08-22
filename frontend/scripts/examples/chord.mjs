// Chord gesture: reveals a Cell, then runs a chord on a revealed numeric
// Cell found from the current state — the pattern for "reproduce a scene".
//
// Usage: node examples/chord.mjs [seed]

import { openshot } from "../shot.mjs";

const seed = process.argv[2] ? Number(process.argv[2]) : undefined;

const shot = await openshot({ seed });
try {
  // Reveal the center; an empty Cell cascades and reveals a region.
  let s = await shot.snapshot();
  await shot.reveal(Math.floor(s.rows / 2), Math.floor(s.cols / 2));

  // Pick a revealed, non-zero numeric Cell to chord on (a real chord target).
  s = await shot.snapshot();
  const idx = s.cells.findIndex(
    (c) =>
      c.state === "revealed" && typeof c.content === "number" && c.content > 0,
  );
  if (idx >= 0) {
    const row = Math.floor(idx / s.cols);
    const col = idx % s.cols;
    console.log(
      `chording on revealed numeric Cell (${row},${col}) content=${s.cells[idx].content}`,
    );
    // press left+right on the Cell, keep the release on it.
    await shot.chord().pressAt(row, col).moveTo(row, col).release();
  } else {
    console.log("no revealed numeric Cell; chord skipped (scene needs one)");
  }

  const p = await shot.screenshot("chord.png");
  console.log("saved", p);
} finally {
  await shot.close();
}
