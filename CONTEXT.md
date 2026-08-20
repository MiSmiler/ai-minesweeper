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
The state of a game: `Ready` (Board exists, no Cell has been Revealed, the Timer has not started), `Playing` (First Click made, in progress), `Won` (all non-Mine Cells Revealed), `Lost` (a Mine was revealed).
_Avoid_: Status, phase

**Mine**:
A Cell that ends the game if Revealed. In Classic Mode, Mines are placed at Game creation and the First Click is unprotected — it may be a Mine; in Prank Mode, the First Clicked Cell is always a Mine.
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
An action on a Revealed numeric Cell: when the number of Flags around it equals the Cell's number, all remaining unflagged neighboring Cells are Revealed. It is armed by holding the Left and Right buttons together (see **Arm**). While armed, moving the pointer over a Revealed numeric Cell shows the Chord Preview, which follows the pointer until Left is released; releasing Left solves the Chord (Reveals the Preview's scope), and the gesture does nothing while the pointer is off a Revealed numeric Cell.
_Avoid_: Combo, sweep

**Arm**:
The act of putting the Chord gesture into its ready state by holding the Left and Right buttons together, in any button order and wherever Right went down. An armed Chord stays inert until the pointer is over a Revealed numeric Cell, where it shows the Chord Preview; when Left is released with a Preview shown, the Chord solves (Reveals), and a plain release just disarms. The gesture disarms when Left is released or the window loses focus; releasing Right alone does not disarm it, and moving the pointer off the Board keeps it armed.
_Avoid_: Trigger, activate, engage

**Chord Preview**:
The transient highlight shown while the Chord gesture is armed: holding Left and Right together and moving the pointer over a Revealed numeric Cell renders all of its unflagged neighboring Cells as empty Revealed Cells, showing the scope of the pending Chord. While Left is held, moving the pointer over other Revealed numeric Cells moves the Preview (and the pending Chord's target) with it; the Preview clears whenever the pointer is not over a Revealed numeric Cell. Releasing Right does not clear it — once armed, the Preview is driven by Left alone. It also clears when Left is released, the window blurs, or the pointer leaves the Board. It is a pure visual — no action is sent — and independent of whether the Chord would actually solve.
_Avoid_: try-chord, preview

**First Click**:
The first Reveal of a game; the Timer starts at it. In Classic Mode it has no protection — it may be a Mine; in Prank Mode it is always a Mine, ending the game immediately in `Lost`.
_Avoid_: Initial click, opening move

**Classic Mode**:
The standard game variant: Mines are placed when the Game is created, the First Click is unprotected (it may be a Mine), and the Seed pins the layout exactly.
_Avoid_: Normal mode, standard mode

**Seed**:
The value fixed when a Game is created that determines the Mine layout. In Classic Mode the layout is fixed at creation, so the Seed alone (with the Difficulty) pins it exactly; in Prank Mode the First Clicked Cell is always a Mine, so the Seed pins the layout only given the First Click. The Seed is a backend detail — the player never sees it — and reproducibility is guaranteed only within the same build.
_Avoid_: Layout seed, RNG key

**Prank Mode**:
A game variant in which the First Click is always a Mine, ending the game immediately in `Lost` — the game is unwinnable by design. Enabled by the `--prank` launch parameter; the UI never indicates it is active.
_Avoid_: Trick mode, joke mode, jinx mode
