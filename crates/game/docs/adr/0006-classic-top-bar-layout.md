# Classic top-bar layout, light gray theme

Issue #15 asked for the layout to move toward the classic Minesweeper look. The interpretation that landed is narrower than a full re-skin: the top bar is restructured into the classic three slots — Flags Remaining counter (left), Smiley Button (center), Timer (right) — with the difficulty selector moved to its own row above, while the board frame and beveled Cells keep their existing shape.

The theme is light gray in the classic style. It was dark originally; once the page background was lightened, the dark game area looked jarring against it, so the game area was switched to a light gray palette too. The black-red LED panels, the digit colors, and the emoji faces are unchanged.

A pixel-faithful Windows XP re-skin was considered and rejected: recognizability, not fidelity, is the goal.

## Considered options

- **System-font displays, not LED fonts.** The counter and timer are three-digit red-on-dark panels, but no LED-style font is loaded — the app's existing font stack renders the digits.
- **Emoji smiley, not pixel art or images.** The Smiley Button's four faces (neutral / surprised / won / lost) are emoji, avoiding custom drawing or asset loading.
- **Seconds Timer.** The Timer displays pure seconds capped at 999 like the classic, replacing the previous MM:SS format.
- **Banner removed.** The WON/LOST banner was removed; the Smiley Button's face communicates the outcome.

## Consequences

The Timer's glossary entry now describes a three-digit seconds display, and a Smiley Button entry was added to CONTEXT.md.
