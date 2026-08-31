# ai-minesweeper

A Minesweeper game with a web UI: a TypeScript frontend over a Rust backend, controlled entirely by mouse.

## Language

**Game**:
A single playthrough created from a Difficulty; the player Reveals Cells until the game is Won or Lost.
_Avoid_: Round, match, session

**Board**:
A rectangular grid of Cells, sized by the active Difficulty. The Cells are interactively contiguous — the hairline between them is decorative and does not affect input: every pointer position on the Board maps to a Cell.
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
The state of a game: `Ready` (Board exists, no Cell has been Revealed, the Timer has not started; the Mines are not yet placed — every game places them at the First Click), `Playing` (First Click made, in progress), `Won` (all non-Mine Cells Revealed), `Lost` (a Mine was revealed). A Won or Lost Game ignores further Gestures — the Board is inert — though the Smiley Button still starts a new Game.
_Avoid_: Status, phase

**Mine**:
A Cell that ends the game if Revealed. A pinned-Seed game reproduces an exact layout and may put a Mine under the First Click; a Random game never places a Mine on or adjacent to the First Clicked Cell (it regenerates the Seed to guarantee this); with the Prank Feature the First Clicked Cell is always a Mine.
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
The act of opening a Hidden Cell, showing its content. Revealing a Mine loses the game; revealing a Cell with zero neighbor mines cascades into the surrounding Cells.
_Avoid_: Open, uncover, click *(as a synonym for Reveal — the player's input gesture is still called a click; see First Click)*

**Flag**:
A marker a player places on a Hidden Cell to indicate a suspected Mine. On a Won board, the game auto-Flags every Mine: player-placed Flags are kept, the rest are marked by the game. Flagged Cells cannot be Revealed. Flags do not affect the win condition — winning only requires all non-Mine Cells to be Revealed. Any Hidden Cell may be Flagged regardless of how many Flags are already placed (Over-flagging); removing a Flag is always allowed.
_Avoid_: Mark, bookmark

**Timer**:
The elapsed time since the First Click, shown in the top bar as a three-digit seconds display (000–999, capping at 999 like the classic); it shows 000 while Ready and freezes when the game ends.
_Avoid_: Clock, stopwatch

**Smiley Button**:
The button in the top bar that starts a new game (the same new-game Action a difficulty switch sends). Its face reflects the game and the player's pressing: normal while Ready/Playing, surprised while pressing on the Board, sunglasses on Won, crying on Lost.
_Avoid_: Face button, reset button

**Gesture**:
A single player input interaction with the mouse over the Board: a press, a move, and a release, in any combination of the Left and Right buttons. Gestures drive the game: a Left press is a Press gesture — it shows the Press Preview and Reveals on release — and holding the Left and Right buttons together is the Chord gesture (see Arm). The frontend translates mouse input into Gestures and tracks each Gesture's Phase.
_Avoid_: click (as a name for a Gesture — see First Click), mouse event

**Gesture Phase**:
The stage a Gesture is in: `idle` — no gesture in progress, though a Right press may be held (its Flag already sent, the press remembered for a possible Chord); `pressing` — a Left press is in progress: the Press Preview follows the pointer, and releasing Left Reveals; `armed` — the Chord gesture is ready: the Chord Preview follows the pointer, and releasing Left Chords. A Gesture's phase is distinct from its button state: a held Right press alone stays `idle`, and releasing Right never leaves `armed`.
_Avoid_: state, status, mode

**Hairline**:
The thin line between Cells showing the Board's surface color; purely visual. The Board is interactively contiguous — pointer positions on the hairline map to a Cell as if the Cells touched, so the hairline never interrupts a Gesture.
_Avoid_: grid line, seam, gap

**Chord**:
An action on a Revealed numeric Cell: when the number of Flags around it equals the Cell's number, all remaining unflagged neighboring Cells are Revealed — each subject to the same cascade rule as a click, so a zero Cell revealed by a Chord cascades into the surrounding Cells. It is armed by holding the Left and Right buttons together (see **Arm**). While armed, moving the pointer over a Revealed numeric Cell shows the Chord Preview, which follows the pointer until Left is released; releasing Left solves the Chord (Reveals the Preview's scope), and the gesture does nothing while the pointer is off a Revealed numeric Cell.
_Avoid_: Combo, sweep

**Arm**:
The act of putting the Chord gesture into its ready state by holding the Left and Right buttons together, in any button order, provided both presses land on Revealed Cells (any Revealed content, not only numeric). An armed Chord shows the Chord Preview while the pointer is over a Revealed numeric Cell — a press that Arms over such a Cell shows it immediately — and stays inert elsewhere; when Left is released with a Preview shown, the Chord solves (Reveals), and a plain release just disarms. The gesture disarms when Left is released or the window loses focus; releasing Right alone does not disarm it, and moving the pointer off the Board keeps it armed — but preview-less: the Chord Preview does not return on re-entry until the gesture re-arms.
_Avoid_: Trigger, activate, engage

**Preview**:
A transient highlight shown during a Gesture; either a Press Preview or a Chord Preview. At most one Preview is active at a time (Arming clears the Press Preview, and entering `pressing` clears the Chord Preview). The frontend models it as a single concept.
_Avoid_: highlight, overlay, preview *(as a synonym — see Press Preview and Chord Preview)*

**Chord Preview**:
The transient highlight shown while the Chord gesture is armed: holding Left and Right together with the pointer over a Revealed numeric Cell renders all of its unflagged neighboring Cells as empty Revealed Cells, showing the scope of the pending Chord — the press that Arms the Chord shows it immediately, in either press order. While Left is held, moving the pointer over other Revealed numeric Cells moves the Preview (and the pending Chord's target) with it; the Preview clears whenever the pointer is not over a Revealed numeric Cell. Releasing Right does not clear it — once armed, the Preview is driven by Left alone. It clears when the window blurs or the pointer leaves the Board — leaving the Board clears it for the rest of the gesture (re-entering does not bring it back until the Chord re-arms); when Left is released with a Preview shown, the Chord solves and the highlight stays visible until the game confirms the Reveal (the re-render that shows the Cells revealed replaces it). It is a pure visual — no action is sent — and independent of whether the Chord would actually solve.
_Avoid_: try-chord, preview

**Press Preview**:
The transient highlight shown while the Left button is held: the Cell under the pointer is highlighted and follows the pointer while Left is held, clears when the pointer leaves the Board, and the Cell under the pointer is Revealed when Left is released (the highlight stays visible until the game confirms the Reveal). It is a pure visual — no action is sent until the release — and distinct from the Chord Preview, which shows a Chord's scope while armed.
_Avoid_: click-preview, hover preview

**First Click**:
The first Reveal of a game; the Timer starts at it, and it places the Mines for every game (Pinned, Random, and Prank alike). A Random game regenerates the Seed so the clicked Cell's 3×3 is Mine-free — the First Click is safe — while a pinned Seed reproduces the exact layout and leaves the First Click unprotected; with the Prank Feature it is always a Mine, ending the game immediately in `Lost`.
_Avoid_: Initial click, opening move

**pinned_seed**:
The optional Seed fixed in `GameConfig` for a game's Mine layout: `Some` pins one Seed so the same Difficulty reproduces the same layout, `None` draws a fresh random Seed per game. Prank is mutually exclusive with a pinned Seed — a Prank game drops any passed Seed — so a pinned Seed is only ever a non-Prank channel. It is a backend detail, never shown to the player. It is the session's intent, set at launch and flowing into every GameConfig, distinct from a Game's committed Seed.
_Avoid_: Seed policy, pinned policy

**committed_seed**:
The Seed actually backing the placed Mine layout, on `Game.committed_seed()`: `Some` exactly when the Mines are placed, `None` while Ready. Every game commits it at the First Click — a pinned Seed to the pinned value, a Random game to the accepted (safe) candidate, a Prank game to a local draw — so a pinned Seed is observable only after the First Click; `committed_seed()` is `None` at Ready for every game.
_Avoid_: Seed (as a synonym for the committed value), replay seed

**Seed**:
The value that pins the Mine layout. It is committed at the First Click: a pinned Seed (`GameConfig.pinned_seed` `Some`) reproduces the exact layout, with the First Click unprotected; a Random game re-draws a candidate at the First Click, regenerating until the clicked Cell's 3×3 is Mine-free, so the committed Seed reproduces that safe board. Before commit a Random game's Seed is only a provisional candidate, not yet the layout key. The Seed is a backend detail — the player never sees it — and reproducibility is guaranteed only within the same build.
_Avoid_: Layout seed, RNG key

**Feature**:
An opt-in behavior that modifies how a game plays, independent of the rule set and of whether a Seed is pinned. The only Feature is Prank.
_Avoid_: Mode, variant, option (when meaning an opt-in behavior)

**Features**:
The set of opt-in behaviors enabled on a Game; presence means a behavior is on. A Game's Features are fixed at creation.
_Avoid_: Options, flags, settings

**Prank**:
A Feature that makes the First Click always a Mine, ending the game immediately in `Lost` — the game is unwinnable by design. It is mutually exclusive with a pinned Seed (Prank is unseedable and non-reproducible), so it never coexists with a `--seed`. Enabled by the `--prank` launch parameter; the UI never indicates it is active.
_Avoid_: Prank mode, trick mode, joke mode, jinx mode

**PlayMode**:
The perspective a single Game is shown from, independent of the rule set and of which Features are on: `SinglePlay` — the player plays unaided; `AiHelpMePlay` — the player plays while DeepSeek watches and suggests. The set is open and may grow (e.g. `AiPlay`, `AiPlayWithMe`); a Game has exactly one PlayMode at a time.
_Avoid_: PlaySurface, view, perspective (when meaning the mode)
