# Seeded pre-placed mines (Classic Mode) with a backend-held Seed

> Amended by ADR-0009 (Classic focus): a random Classic game no longer places
> its Mines at creation — it defers to the First Click and regenerates the Seed
> until the clicked Cell's 3×3 is Mine-free. This record's "Mines placed at
> creation" and "First Click unprotected" now apply only to a pinned `--seed`
> (the exact-layout reproduction channel); relocation remains rejected.

Classic Mode now places its full Mine set at Game creation, sampled from the entire Board by a Seed-seeded RNG with no protection for the First Click — matching classic Minesweeper and making the Mine layout a pure function of (Seed, Difficulty), so the same Seed and Difficulty reproduce the same layout. The Seed is a backend detail: a launch parameter pins one Seed for the whole session, otherwise every Game draws a fresh random Seed printed to the terminal; it never reaches the wire or the UI. Prank Mode is unchanged (ADR-0002) — Mines are still placed at the First Click with the clicked Cell forced into the Mine list — so its layout is a function of (Seed, Difficulty, First Click) and reproducing it requires the same First Click.

This amends ADR-0001 for Classic Mode: Mines are now placed at creation rather than on the First Reveal, and the First Click is unprotected. The old guarantee ("a Mine never existed where the player clicked") no longer holds for Classic — the First Click may hit a Mine and lose instantly. Relocation (moving a Mine under the First Click afterwards) stays rejected; forced inclusion at placement keeps Prank's layout honest.

Consequences: reproducibility is per-build only (upgrading rand or changing the sampling algorithm may change which layout a Seed yields); Prank Seed sharing is inherently lossy (the mode is invisible to the UI); and Mine sampling draws distinct Cells by index sampling from a Seed-seeded RNG rather than enumerating and shuffling the whole Board.
