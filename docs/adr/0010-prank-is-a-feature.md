# Prank is a Feature, not a GameMode

Prank was modeled as the `Prank` variant of `GameMode` alongside `Classic`, which conflated an opt-in behavior with the rule set. We removed `GameMode` entirely — the only rule set is the standard one — and model prank as a `Feature` in a `Features` set, so "which Features a game has" and "which SeedPolicy is in play" are two separate, tellable things rather than values of a single `GameMode`.

Considered options: keeping `GameMode` with a single `Classic` value was rejected — a single-variant enum is redundant once prank is no longer a switch; modeling the first-click outcome as a `FirstClickPolicy` enum (`Safe` / `Unprotected` / `AlwaysMine`) was rejected because "AlwaysMine" is exactly prank's only effect, which would re-conflate it with a rule axis; making Seed a `Feature` was rejected because Features are presence-toggles while a Seed carries a value and is the reproducibility channel.

Consequences: there is no "game mode" dimension anymore — the game is unambiguously standard Minesweeper plus optional Features and a SeedPolicy. The derived first-click safety is now `SeedPolicy::Random ∧ ¬Prank`. ADR-0002's behavior is unchanged but reframed in Feature terms; ADR-0009's `first_click_safe` field is replaced by the explicit `SeedPolicy`.
