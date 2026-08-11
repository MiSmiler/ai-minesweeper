# ai-minesweeper

A terminal Minesweeper game rendered with ratatui, controlled entirely by mouse.

## Language

**Board**:
A rectangular grid of Cells, sized by the active Difficulty.
_Avoid_: Grid, field, minefield

**Cell**:
A single square of the Board. A Cell either contains a Mine or a neighbor count (the number of Mines in its 8 surrounding Cells).
_Avoid_: Tile, square, block

**Difficulty**:
One of three classic presets: Beginner (9×9, 10 mines), Intermediate (16×16, 40 mines), Expert (30×16, 99 mines).

**CellState**:
The player-visible state of a Cell: `Hidden` (not yet touched), `Revealed` (shows its content), or `Flagged` (marked by the player as suspected Mine). There is no question-mark state.
_Avoid_: Unopened, opened, marked

**GameState**:
The state of a game: `Ready` (Board exists but mines are not yet placed), `Playing` (mines placed, in progress), `Won` (all non-Mine Cells Revealed), `Lost` (a Mine was revealed).
_Avoid_: Status, phase

**Mine**:
A Cell that ends the game if Revealed. Mines are never placed on or adjacent to the first-clicked Cell.
_Avoid_: Bomb

**Mine Counter**:
The player-facing number of total Mines minus placed Flags. It never goes below zero — Flagging is refused once it reaches zero, so Flags can never exceed the total Mine count.
_Avoid_: Remaining mines, mines left

**Trigger Mine**:
The Mine that was Revealed to cause `Lost`; it is highlighted in the final board reveal.

**Reveal**:
The act of opening a Hidden Cell, showing its content. Revealing a Mine loses the game; revealing a Cell with zero neighbor mines flood-fills the surrounding Cells.
_Avoid_: Open, uncover, click

**Flag**:
A marker a player places on a Hidden Cell to indicate a suspected Mine. Flagged Cells cannot be Revealed. Flags do not affect the win condition — winning only requires all non-Mine Cells to be Revealed. The number of Flags can never exceed the total Mine count: once the Mine Counter reaches zero, placing further Flags is refused (removing a Flag is always allowed).
_Avoid_: Mark, bookmark

**Chord**:
An action on a Revealed numeric Cell: when the number of Flags around it equals its count, all remaining unflagged neighboring Cells are Revealed. Triggered by pressing Left while Right is held, only on Revealed numeric Cells; anywhere else it does nothing.
_Avoid_: Combo, sweep

**First Click**:
The first Reveal of a game. It is always safe: mines are placed only after it, never on or adjacent to the clicked Cell.
_Avoid_: Initial click, opening move
