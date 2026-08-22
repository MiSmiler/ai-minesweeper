// Minimal scene: reveal a couple of Cells, then screenshot the board.
//
// Usage: node examples/basic.mjs [seed]
//   Pass an optional seed to pin the board layout (only applies when the
//   script starts its own backend — a reused service keeps its own layout).

import { openshot } from "../shot.mjs";

const seed = process.argv[2] ? Number(process.argv[2]) : undefined;

const shot = await openshot({ seed });
try {
  const s = await shot.snapshot();
  console.log(
    `board ${s.rows}x${s.cols}, state=${s.game_state}, ` +
      `flags_remaining=${s.flags_remaining}`,
  );

  // Reveal the middle Cells; an empty one cascades and reveals its region.
  await shot.reveal(Math.floor(s.rows / 2), Math.floor(s.cols / 2));

  const p = await shot.screenshot("basic.png");
  console.log("saved", p);
} finally {
  await shot.close();
}
