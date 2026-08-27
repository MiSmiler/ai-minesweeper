# Prank Feature: the first click is always a mine

Prank is a deliberate joke feature: enabled by the `--prank` launch parameter, the First Click of every game reveals a Mine and loses instantly — the game is unwinnable by design, and the UI gives no hint that the feature is active. The first-clicked Cell is forced into the Mine list at placement time, with the remaining Mines sampled randomly from the rest of the board — rather than placing Mines normally and relocating one under the first click afterwards. Forced inclusion keeps the deferred-placement architecture of ADR-0001 intact (Mines never exist until the First Reveal): where a `Random` SeedPolicy guarantees a Mine never was under the First Click, Prank guarantees one always was. Trade-offs: Prank games cannot be Won, and the first-click safety property of ADR-0001 holds only when Prank is off.

Amended by ADR-0010: Prank is modeled as a Feature, not a `GameMode`; the behavior above is unchanged.
