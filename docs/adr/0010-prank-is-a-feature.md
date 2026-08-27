# Prank is a Feature, not a GameMode

Prank was modeled as the `Prank` variant of `GameMode` alongside `Classic`, which conflated an opt-in behavior with the rule set. We removed `GameMode` entirely — the only rule set is the standard one — and model prank as a `Feature` in a `Features` set, so "which Features a game has" and "whether a Seed is pinned" are two separate, tellable things rather than values of a single `GameMode`.

Considered options: keeping `GameMode` with a single `Classic` value was rejected — a single-variant enum is redundant once prank is no longer a switch; modeling the first-click outcome as a `FirstClickPolicy` enum (`Safe` / `Unprotected` / `AlwaysMine`) was rejected because "AlwaysMine" is exactly prank's only effect, which would re-conflate it with a rule axis; making Seed a `Feature` was rejected because Features are presence-toggles while a Seed carries a value and is the reproducibility channel.

Consequences: there is no "game mode" dimension anymore — the game is unambiguously standard Minesweeper plus optional Features and a possibly-pinned Seed. ADR-0002's behavior is unchanged but reframed in Feature terms.

Amended by the seed-model rework (#105/#102): the old `Pinned`/`Random` policy is gone. `GameConfig` carries `features` and `pinned_seed: Option<Seed>`. Prank is mutually exclusive with a pinned Seed — `GameConfig::new` drops a passed Seed for a Prank game — so a Prank game is non-reproducible. Every game places its Mines at the First Click; `Game.committed_seed()` is `Some` exactly when Mines are placed (a pinned game commits to the pinned value, a Random game to the accepted candidate, a Prank game to a local draw). The derived first-click safety is `pinned_seed == None ∧ ¬Prank`.
