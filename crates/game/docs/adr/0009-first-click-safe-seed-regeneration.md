# First-click safety by Seed regeneration in Classic Mode

A random game's First Click is safe: the Seed is re-drawn at the First Click and regenerated until the clicked Cell's 3×3 is Mine-free, and the accepted Seed becomes the layout key. A pinned `--seed` keeps the exact-layout reproduction channel, so seeding stays a pure reproducibility channel and the First Click may be unprotected there; reproduction and first-click safety are therefore split between the pinned and random paths.

Considered options: seed-preserving deferred placement (the layout a function of Seed + First Click, borrowed from Prank) was rejected because Prank is a joke variant and a Seed must alone reproduce a board; relocating a Mine under the First Click (ADR-0004) stays rejected.

Consequences: a random game's Seed is not fixed at creation — the accepted Seed, chosen at the First Click, is logged for replay — while a pinned `--seed` still reproduces a layout exactly; a game's `Ready` state has no Mines yet (placement is deferred for every game); reproducibility is per-build only, and a random game's First Click is safe only on a mine-free 3×3.

Amended: the model keeps two orthogonal axes that must not be conflated — whether a Seed value exists (a random game picks a *candidate* at the First Click, re-drawn until the click is safe) and whether it is the authoritative layout key (only after Mines are placed). `GameConfig.pinned_seed: Option<Seed>` and the plain field `Game.committed_seed: Option<Seed>` carry these axes; a Random game is `pinned_seed: None`. The write side is `Game::commit_mines` (the single seam that fixes a Mine list and thereby commits the Seed); the read side is `Game::committed_seed()` (`Some` once the layout is fixed, `None` before). The provisional candidate is never surfaced to callers.

Amended by ADR-0010 and the seed-model rework (#105/#102): every game — Pinned, Random, and Prank alike — now uses a `pinned_seed`/`committed_seed` pair rather than a `Pinned`/`Random` policy; even a pinned game defers its Mines to the First Click, so `committed_seed()` is `None` at `Ready` for every game. The derived first-click safety is `pinned_seed == None ∧ ¬Prank`.
