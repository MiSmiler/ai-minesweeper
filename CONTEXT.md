# ai-minesweeper

A Minesweeper game with a web UI: a TypeScript frontend over a Rust backend, controlled entirely by mouse.

## Language

**Game**:
A single playthrough created from a Difficulty; the player Reveals Cells until the game is Won or Lost.
_Avoid_: Round, match, session

**Board**:
A rectangular grid of Cells, sized by the active Difficulty.
_Avoid_: Grid, field, minefield

**Cell**:
A single square of the Board, identified by its Position. A Cell either contains a Mine or a neighbor count (the number of Mines in its 8 surrounding Cells).
_Avoid_: Tile, square, block

**Position**:
The coordinates of a Cell on the Board, in (row, col) order; the way a Cell is addressed.
_Avoid_: cell (as a name for coordinates), coords, point

**Difficulty**:
One of three classic presets: Beginner (9×9, 10 mines), Intermediate (16×16, 40 mines), Expert (30×16, 99 mines).

**CellState**:
The player-visible state of a Cell: `Hidden` (not yet touched), `Revealed` (shows its content), or `Flagged` (marked as a suspected Mine — by the player during play, or by the game on a Won board). There is no question-mark state.
_Avoid_: Unopened, opened, marked

**GameState**:
The state of a game: `Ready` (Board exists but mines are not yet placed), `Playing` (mines placed, in progress), `Won` (all non-Mine Cells Revealed), `Lost` (a Mine was revealed).
_Avoid_: Status, phase

**Mine**:
A Cell that ends the game if Revealed. Mines are never placed on or adjacent to the first-clicked Cell.
_Avoid_: Bomb

**Flag Budget**:
The zero reference of the Flag counter, equal to the Difficulty's Mine count; fixed at game start and never changes during the game. It is not a cap: the player may place more Flags than the Flag Budget (Over-flagging).
_Avoid_: Mine budget, flag quota

**Flags Remaining**:
The Flag Budget minus the number of placed Flags. It goes negative when the player has over-flagged and rises back toward zero as Flags are removed.
_Avoid_: Mine counter, mines remaining, mines left

**Over-flagging**:
Placing more Flags than the Flag Budget, driving Flags Remaining negative; a deliberate technique for temporary marking while reasoning about the Board.
_Avoid_: Overdraft, overflagging

**Trigger Mine**:
The Mine that was Revealed to cause `Lost`; it is highlighted in the final board reveal.

**Reveal**:
The act of opening a Hidden Cell, showing its content. Revealing a Mine loses the game; revealing a Cell with zero neighbor mines flood-fills the surrounding Cells.
_Avoid_: Open, uncover, click *(as a synonym for Reveal — the player's input gesture is still called a click; see First Click)*

**Flag**:
A marker a player places on a Hidden Cell to indicate a suspected Mine. On a Won board, the game auto-Flags every Mine: player-placed Flags are kept, the rest are marked by the game. Flagged Cells cannot be Revealed. Flags do not affect the win condition — winning only requires all non-Mine Cells to be Revealed. Any Hidden Cell may be Flagged regardless of how many Flags are already placed (Over-flagging); removing a Flag is always allowed.
_Avoid_: Mark, bookmark

**Timer**:
The elapsed time since the First Click, shown in the top bar; it shows 00:00 while Ready and freezes when the game ends.
_Avoid_: Clock, stopwatch

**Chord**:
An action on a Revealed numeric Cell: when the number of Flags around it equals the Cell's number, all remaining unflagged neighboring Cells are Revealed. Triggered by pressing Left while Right is held, only on Revealed numeric Cells; anywhere else it does nothing.
_Avoid_: Combo, sweep

**First Click**:
The first Reveal of a game. It is always safe, except in Prank Mode: mines are placed only after it, never on or adjacent to the clicked Cell.
_Avoid_: Initial click, opening move

**Prank Mode**:
A game variant in which the First Click is always a Mine, ending the game immediately in `Lost` — the game is unwinnable by design. Enabled by the `--prank` launch parameter; the UI never indicates it is active.
_Avoid_: Trick mode, joke mode, jinx mode
