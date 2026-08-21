# Deferred mine placement

> Amended by ADR-0004 (Classic Mode): Classic places its Mines at Game
> creation; the deferred, first-click-safe placement now applies to Prank
> Mode only (ADR-0002).
>
> Terminology note (issue #25): the glossary later renamed the
> "flood-fill" mechanic to "cascade"; this record keeps the original term.

Mines are placed only after the first click of a game, never on or adjacent to the first-clicked cell — rather than generating them at board creation and relocating any mine hit by the first click. This makes the first click strictly safe ("a mine never existed where the player clicked") instead of retroactively moved, gives the classic zero-neighbor flood-fill opening, and pairs naturally with the `Ready` game state. Trade-off: the board must expose mines as a hidden detail until `GameState` leaves `Ready`, and layout code must tolerate a board that briefly has no mines.
