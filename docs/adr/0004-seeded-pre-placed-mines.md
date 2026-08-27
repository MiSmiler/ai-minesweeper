# Seeded pre-placed mines (Classic Mode) with a backend-held Seed

> Amended by the seed-model rework (#105/#102): placement is now deferred to
> the First Click for every game, including a pinned `--seed`. A pinned game
> no longer places its Mines at creation — it samples the exact layout from
> the Seed at the First Click and commits it to `Game.committed_seed()`
> (`Some` exactly when Mines are placed). The old `Pinned`/`Random` policy is
> gone: `GameConfig.pinned_seed: Option<Seed>` is the channel (a pinned Seed
> is `Some`, a fresh Random game `None`). The "First Click unprotected" property
> still applies to a pinned layout; relocation remains rejected.

> Amended by ADR-0009 (Classic focus): a random Classic game no longer places
> its Mines at creation — it defers to the First Click and regenerates the Seed
> until the clicked Cell's 3×3 is Mine-free. This record's "Mines placed at
> creation" and "First Click unprotected" now apply only to a pinned `--seed`
> (the exact-layout reproduction channel); relocation remains rejected.

Classic Mode (a pinned `--seed`) places its full Mine set at the First Click, sampled from the entire Board by a Seed-seeded RNG with no protection for the First Click — matching classic Minesweeper and making the Mine layout a pure function of (Seed, Difficulty), so the same Seed and Difficulty reproduce the same layout. The Seed is a backend detail: a launch parameter pins one Seed for the whole session, otherwise every Game draws a fresh random Seed printed to the terminal; it never reaches the wire or the UI. Prank is unchanged (ADR-0002) — its Mines are placed at the First Click with the clicked Cell forced into the Mine list — and it drops its Seed, so its layout is a function of (Difficulty, First Click) and non-reproducible.

This amends ADR-0001 for Classic Mode: Mines are now placed on the First Reveal rather than at creation, and the First Click is unprotected. The old guarantee ("a Mine never existed where the player clicked") no longer holds for Classic — the First Click may hit a Mine and lose instantly. Relocation (moving a Mine under the First Click afterwards) stays rejected; forced inclusion at placement keeps Prank's layout honest.

Consequences: reproducibility is per-build only (upgrading rand or changing the sampling algorithm may change which layout a Seed yields); Prank Seed sharing is inherently lossy (the mode is invisible to the UI); and Mine sampling draws distinct Cells by index sampling from a Seed-seeded RNG rather than enumerating and shuffling the whole Board.
