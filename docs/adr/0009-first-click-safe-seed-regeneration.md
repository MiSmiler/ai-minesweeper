# First-click safety by Seed regeneration in Classic Mode

Classic Mode so far pre-placed its Mines at Game creation (ADR-0004), leaving the First Click unprotected. It now makes a random game's First Click safe: the Seed is re-drawn at the First Click and regenerated until the clicked Cell's 3×3 is Mine-free, and the accepted Seed becomes the layout key. A pinned `--seed` keeps the old semantics — the full Mine set placed at creation and reproduced exactly — so seeding stays a pure reproducibility channel and the First Click may be unprotected there; reproduction and first-click safety are therefore split between the pinned and random paths.

Considered options: seed-preserving deferred placement (the layout a function of Seed + First Click, borrowed from Prank) was rejected because Prank is a joke variant and a Seed must alone reproduce a board; relocating a Mine under the First Click (ADR-0004) stays rejected.

Consequences: a random game's Seed is not fixed at creation — the accepted Seed, chosen at the First Click, is logged for replay — while `Game::with_seed` (a pinned seed) still reproduces a layout exactly; a random game's `Ready` state may have no Mines yet; reproducibility is per-build only, and a random game's First Click is safe only on a mine-free 3×3.
