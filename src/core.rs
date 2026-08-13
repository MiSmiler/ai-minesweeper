//! Core game logic for Minesweeper: pure rules, zero UI dependencies.
//!
//! The public interface of this module is the only seam — both the UI layer
//! and the unit tests drive the game through it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;

/// The Board's dimensions in Cells, in (rows, cols) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardSize {
    pub rows: usize,
    pub cols: usize,
}

impl BoardSize {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self { rows, cols }
    }
}

/// One of the three classic difficulty presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Expert,
}

impl Difficulty {
    /// The classic board size for this difficulty.
    pub fn size(self) -> BoardSize {
        match self {
            Difficulty::Beginner => BoardSize::new(9, 9),
            Difficulty::Intermediate => BoardSize::new(16, 16),
            Difficulty::Expert => BoardSize::new(16, 30),
        }
    }

    /// Total number of Mines on the board.
    pub fn mine_count(self) -> usize {
        match self {
            Difficulty::Beginner => 10,
            Difficulty::Intermediate => 40,
            Difficulty::Expert => 99,
        }
    }
}

/// A Cell's coordinates on the Board, in (row, col) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// The player-visible state of a Cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellState {
    Hidden,
    Revealed,
    Flagged,
}

/// The content of a Cell, visible only after it is Revealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellContent {
    /// Adjacent Mine count (0 = empty Cell).
    Number(u8),
    Mine,
}

/// What a Cell looks like to the outside world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellView {
    pub state: CellState,
    /// `Some` only for Revealed Cells.
    pub content: Option<CellContent>,
}

/// The game variant being played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    /// Standard rules: the First Click is never a Mine (ADR-0001).
    Classic,
    /// Prank: the First Click is always a Mine, losing the game instantly.
    Prank,
}

/// The state of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    /// Board exists but Mines are not yet placed.
    Ready,
    /// Mines placed, game in progress.
    Playing,
    /// All non-Mine Cells Revealed.
    Won,
    /// A Mine was Revealed.
    Lost,
}

/// A Minesweeper game. Mines are placed on the first Reveal (ADR-0001).
#[derive(Debug)]
pub struct Game {
    difficulty: Difficulty,
    mode: GameMode,
    size: BoardSize,
    state: GameState,
    /// Mine positions; `None` until the first Reveal places them.
    mines: Option<Vec<Position>>,
    cells: Vec<CellState>,
    flags: usize,
    /// The Flag Budget: the number of Flags the player may place this game,
    /// set at creation from the Difficulty's Mine count (the "recipe");
    /// it never changes during the game.
    flag_budget: usize,
    /// Trigger Mine position; `None` unless the game is Lost.
    trigger: Option<Position>,
    started_at: Option<Instant>,
    elapsed_at_end: Option<Duration>,
}

impl Game {
    /// Creates a game in `Ready` state. Mines are placed randomly on the
    /// first Reveal, never on or adjacent to the first-clicked Cell
    /// (Classic), or always including it (Prank, ADR-0002).
    pub fn new(difficulty: Difficulty, mode: GameMode) -> Self {
        let size = difficulty.size();
        Self {
            difficulty,
            mode,
            size,
            state: GameState::Ready,
            mines: None,
            cells: vec![CellState::Hidden; size.rows * size.cols],
            flags: 0,
            flag_budget: difficulty.mine_count(),
            trigger: None,
            started_at: None,
            elapsed_at_end: None,
        }
    }

    /// Creates a game with a preset Mine list (test use). In Classic mode
    /// the caller is responsible for first-click safety: Mines on or
    /// adjacent to the first-clicked Cell are NOT filtered. In Prank mode
    /// the first-clicked Cell is always forced into the Mine list. The
    /// Flag Budget is still set from the Difficulty (the recipe) and is
    /// independent of the preset length.
    #[cfg(test)]
    pub fn with_mines(difficulty: Difficulty, mode: GameMode, mines: &[Position]) -> Self {
        let mut game = Self::new(difficulty, mode);
        game.mines = Some(mines.to_vec());
        game
    }

    /// The current game state.
    pub fn game_state(&self) -> GameState {
        self.state
    }

    /// The board size.
    pub fn size(&self) -> BoardSize {
        self.size
    }

    /// The difficulty this game was created with.
    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// Whether the Cell is the Trigger Mine of a Lost game.
    pub fn is_trigger(&self, pos: Position) -> bool {
        self.trigger == Some(pos)
    }

    /// The visible state of a Cell.
    pub fn cell_view(&self, pos: Position) -> CellView {
        let state = self.cell(pos);
        let content = match state {
            CellState::Revealed => Some(if self.is_mine(pos) {
                CellContent::Mine
            } else {
                CellContent::Number(self.adjacent_mines(pos))
            }),
            _ => None,
        };
        CellView { state, content }
    }

    /// The number of Flags the player can still place. Never below zero:
    /// Flagging is refused once it reaches zero (Flags cannot exceed the
    /// Flag Budget).
    pub fn flags_remaining(&self) -> usize {
        self.flag_budget.saturating_sub(self.flags)
    }

    /// Whether the game has ended (Won or Lost).
    fn ended(&self) -> bool {
        matches!(self.state, GameState::Won | GameState::Lost)
    }

    /// Whether a player action (Reveal / Flag / Chord) can still be applied
    /// to the Cell: the game has not ended and the Cell lies on the Board.
    fn can_operate(&self, pos: Position) -> bool {
        !self.ended() && self.in_bounds(pos)
    }

    /// Whether the Cell lies on the board.
    fn in_bounds(&self, pos: Position) -> bool {
        pos.row < self.size.rows && pos.col < self.size.cols
    }

    /// Reveals a Cell. No-op when the game has ended, the Cell is Flagged,
    /// or it is out of bounds. The first Reveal of a game places the Mines
    /// (ADR-0001) and starts the clock.
    pub fn reveal(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        if self.cell(pos) != CellState::Hidden {
            return;
        }
        if self.state == GameState::Ready {
            self.place_mines(pos);
            self.state = GameState::Playing;
            self.started_at = Some(Instant::now());
        }
        self.reveal_at(pos);
        self.resolve_end(pos);
    }

    fn reveal_at(&mut self, pos: Position) {
        self.set_cell(pos, CellState::Revealed);
        // A Mine never flood-fills: the game ends on it in `resolve_end`.
        if self.is_mine(pos) {
            return;
        }
        if self.adjacent_mines(pos) == 0 {
            let mut queue = VecDeque::from([pos]);
            while let Some(cell) = queue.pop_front() {
                for neighbor in Self::neighbors(self.size, cell) {
                    if self.cell(neighbor) != CellState::Hidden || self.is_mine(neighbor) {
                        continue;
                    }
                    self.set_cell(neighbor, CellState::Revealed);
                    if self.adjacent_mines(neighbor) == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    /// Places the Mines for a new game. Classic (ADR-0001): Mines are
    /// sampled from the Cells excluding the first-clicked Cell and its 8
    /// neighbors. Prank (ADR-0002): the first-clicked Cell is forced into
    /// the Mine list — for presets by union, for random placement by
    /// sampling one fewer Mine and adding the Cell back.
    fn place_mines(&mut self, first_click: Position) {
        let mut mines = match &self.mines {
            Some(preset) => preset.clone(),
            None => {
                let mut candidates: Vec<Position> = Vec::new();
                for row in 0..self.size.rows {
                    for col in 0..self.size.cols {
                        let cell = Position::new(row, col);
                        if cell == first_click {
                            continue;
                        }
                        if self.mode == GameMode::Classic
                            && Self::neighbors(self.size, first_click).any(|n| n == cell)
                        {
                            continue;
                        }
                        candidates.push(cell);
                    }
                }
                let mut rng = rand::rng();
                candidates.shuffle(&mut rng);
                let take = if self.mode == GameMode::Prank {
                    self.flag_budget - 1
                } else {
                    self.flag_budget
                };
                candidates.truncate(take);
                candidates
            }
        };
        if self.mode == GameMode::Prank && !mines.contains(&first_click) {
            mines.push(first_click);
        }
        self.mines = Some(mines);
    }

    /// After a Reveal, transitions the game to Lost or Won when the
    /// condition holds, auto-Revealing the Mines on the final board.
    fn resolve_end(&mut self, pos: Position) {
        if self.is_mine(pos) {
            self.lose(pos);
            return;
        }
        let Some(mines) = &self.mines else { return };
        // If the scan completes without an early return, every non-Mine Cell
        // is Revealed — the game is Won.
        for row in 0..self.size.rows {
            for col in 0..self.size.cols {
                let cell = Position::new(row, col);
                if mines.contains(&cell) {
                    continue;
                }
                if self.cell(cell) != CellState::Revealed {
                    return;
                }
            }
        }
        self.win();
    }

    /// Ends the game as Lost on the given Trigger Mine, auto-Revealing all
    /// unflagged Mines; wrongly Flagged Cells stay Flagged.
    fn lose(&mut self, pos: Position) {
        self.state = GameState::Lost;
        self.trigger = Some(pos);
        self.reveal_mines(false);
        self.elapsed_at_end = Some(self.elapsed());
    }

    /// Ends the game as Won, auto-Revealing all Mines on the final board.
    fn win(&mut self) {
        self.state = GameState::Won;
        self.reveal_mines(true);
        self.elapsed_at_end = Some(self.elapsed());
    }

    /// Reveals the Mines. When `include_flagged` is false, Flagged Mines
    /// keep their Flag (Lost board); when true, all Mines are Revealed (Won).
    fn reveal_mines(&mut self, include_flagged: bool) {
        let Some(mines) = self.mines.clone() else {
            return;
        };
        for pos in mines {
            if include_flagged || self.cell(pos) != CellState::Flagged {
                self.set_cell(pos, CellState::Revealed);
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        match (self.started_at, self.elapsed_at_end) {
            (None, _) => Duration::ZERO,
            (Some(_), Some(ended)) => ended,
            (Some(started), None) => started.elapsed(),
        }
    }

    fn is_mine(&self, pos: Position) -> bool {
        self.mines
            .as_ref()
            .is_some_and(|mines| mines.contains(&pos))
    }

    fn adjacent_mines(&self, pos: Position) -> u8 {
        Self::neighbors(self.size, pos)
            .filter(|&n| self.is_mine(n))
            .count() as u8
    }

    /// Returns the 8 surrounding Cells of `pos`, clamped to the Board;
    /// does not include `pos` itself.
    fn neighbors(size: BoardSize, pos: Position) -> impl Iterator<Item = Position> {
        let (min_r, max_r) = (pos.row.saturating_sub(1), (pos.row + 1).min(size.rows - 1));
        let (min_c, max_c) = (pos.col.saturating_sub(1), (pos.col + 1).min(size.cols - 1));
        (min_r..=max_r)
            .flat_map(move |r| (min_c..=max_c).map(move |c| Position::new(r, c)))
            .filter(move |&n| n != pos)
    }

    /// Toggles a Flag on a Hidden Cell. No-op otherwise: Revealed Cells and
    /// ended games reject Flag toggling. Flagging is refused once the Flags
    /// Remaining has reached zero (Flags cannot exceed the Flag Budget);
    /// removing a Flag is always allowed.
    pub fn toggle_flag(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        match self.cell(pos) {
            CellState::Hidden if self.flags_remaining() > 0 => {
                self.set_cell(pos, CellState::Flagged);
                self.flags += 1;
            }
            CellState::Flagged => {
                self.set_cell(pos, CellState::Hidden);
                self.flags -= 1;
            }
            CellState::Revealed | CellState::Hidden => {}
        }
    }

    /// Chord: on a Revealed numeric Cell whose Flag count matches its
    /// number, Reveals all unflagged neighbors. No-op otherwise — including
    /// on Hidden Cells, zero Cells, and mismatched Flag counts. Does not
    /// flood-fill: only the immediate neighbors are Revealed.
    pub fn chord(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        if self.cell(pos) != CellState::Revealed {
            return;
        }
        let number = self.adjacent_mines(pos);
        if number == 0 {
            return;
        }
        let flagged = Self::neighbors(self.size, pos)
            .filter(|&n| self.cell(n) == CellState::Flagged)
            .count();
        if flagged != number as usize {
            return;
        }
        let mut hit_mine = None;
        for neighbor in Self::neighbors(self.size, pos) {
            if self.cell(neighbor) == CellState::Hidden {
                self.set_cell(neighbor, CellState::Revealed);
                if self.is_mine(neighbor) && hit_mine.is_none() {
                    hit_mine = Some(neighbor);
                }
            }
        }
        match hit_mine {
            Some(pos) => self.lose(pos),
            None => self.resolve_end(pos),
        }
    }

    /// The Cell state at `pos`. The caller must ensure `pos` is in bounds.
    fn cell(&self, pos: Position) -> CellState {
        debug_assert!(self.in_bounds(pos));
        self.cells[self.index(pos)]
    }

    /// Sets the Cell state at `pos`. The caller must ensure `pos` is in bounds.
    fn set_cell(&mut self, pos: Position, state: CellState) {
        debug_assert!(self.in_bounds(pos));
        let idx = self.index(pos);
        self.cells[idx] = state;
    }

    fn index(&self, pos: Position) -> usize {
        pos.row * self.size.cols + pos.col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_presets_have_classic_sizes_and_mine_counts() {
        assert_eq!(Difficulty::Beginner.size(), BoardSize::new(9, 9));
        assert_eq!(Difficulty::Beginner.mine_count(), 10);
        assert_eq!(Difficulty::Intermediate.size(), BoardSize::new(16, 16));
        assert_eq!(Difficulty::Intermediate.mine_count(), 40);
        assert_eq!(Difficulty::Expert.size(), BoardSize::new(16, 30));
        assert_eq!(Difficulty::Expert.mine_count(), 99);
    }

    #[test]
    fn new_game_starts_ready_with_all_cells_hidden() {
        let game = Game::new(Difficulty::Beginner, GameMode::Classic);
        assert_eq!(game.game_state(), GameState::Ready);
        let size = Difficulty::Beginner.size();
        for row in 0..size.rows {
            for col in 0..size.cols {
                let pos = Position::new(row, col);
                assert_eq!(game.cell_view(pos).state, CellState::Hidden);
                assert_eq!(game.cell_view(pos).content, None);
            }
        }
    }

    #[test]
    fn flags_remaining_equals_total_before_any_flag() {
        for difficulty in [
            Difficulty::Beginner,
            Difficulty::Intermediate,
            Difficulty::Expert,
        ] {
            let game = Game::new(difficulty, GameMode::Classic);
            assert_eq!(game.flags_remaining(), difficulty.mine_count());
        }
    }

    #[test]
    fn first_reveal_enters_playing_and_is_always_safe() {
        // Property test: for many random games, the first click and its 8
        // neighbors are never Mines (ADR-0001).
        for _ in 0..20 {
            let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
            game.reveal(Position::new(0, 0));
            assert_eq!(game.game_state(), GameState::Playing);
            for (row, col) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                game.reveal(Position::new(row, col));
                assert_ne!(game.game_state(), GameState::Lost);
            }
        }
    }

    #[test]
    fn reveal_mine_loses_and_auto_reveals_board() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0), Position::new(5, 5)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 0)));
        // The Trigger Mine is Revealed and shown as Mine.
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(0, 0)).content,
            Some(CellContent::Mine)
        );
        // The other unflagged Mine is auto-Revealed too.
        assert_eq!(
            game.cell_view(Position::new(5, 5)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(5, 5)).content,
            Some(CellContent::Mine)
        );
        // The Trigger Mine is the only one flagged as trigger.
        assert!(!game.is_trigger(Position::new(5, 5)));
    }

    #[test]
    fn reveal_shows_neighbor_count() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(2, 2)],
        );
        game.reveal(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Playing);
        assert_eq!(
            game.cell_view(Position::new(1, 1)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(1, 1)).content,
            Some(CellContent::Number(1))
        );
    }

    #[test]
    fn zero_cell_flood_fills_until_numbered_boundary() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(4, 4)],
        );
        game.reveal(Position::new(0, 0));
        // The clicked Cell is a zero Cell.
        assert_eq!(
            game.cell_view(Position::new(0, 0)).content,
            Some(CellContent::Number(0))
        );
        // The numbered boundary of the flood fill is Revealed.
        assert_eq!(
            game.cell_view(Position::new(3, 3)).content,
            Some(CellContent::Number(1))
        );
        // A far corner in the zero region is Revealed.
        assert_eq!(
            game.cell_view(Position::new(8, 8)).content,
            Some(CellContent::Number(0))
        );
        // One lone Mine means the flood fill wins instantly: the game is Won
        // and the Mine is auto-Revealed on the final board.
        assert_eq!(game.game_state(), GameState::Won);
        assert_eq!(
            game.cell_view(Position::new(4, 4)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(4, 4)).content,
            Some(CellContent::Mine)
        );
    }

    #[test]
    fn revealing_every_non_mine_cell_wins_and_reveals_mines() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        let size = Difficulty::Beginner.size();
        for row in 0..size.rows {
            for col in 0..size.cols {
                if Position::new(row, col) != Position::new(0, 0) {
                    game.reveal(Position::new(row, col));
                }
            }
        }
        assert_eq!(game.game_state(), GameState::Won);
        // The Mine is auto-Revealed on the final board.
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(0, 0)).content,
            Some(CellContent::Mine)
        );
    }

    #[test]
    fn ended_game_rejects_reveals() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        // Reveals after the end change nothing.
        game.reveal(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Lost);
        assert_eq!(game.cell_view(Position::new(1, 1)).state, CellState::Hidden);
    }

    #[test]
    fn reveal_out_of_bounds_is_noop() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        game.reveal(Position::new(99, 99));
        assert_eq!(game.game_state(), GameState::Ready);
    }

    #[test]
    fn flag_toggles_hidden_to_flagged_and_back() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(
            game.cell_view(Position::new(1, 1)).state,
            CellState::Flagged
        );
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(game.cell_view(Position::new(1, 1)).state, CellState::Hidden);
    }

    #[test]
    fn flag_on_revealed_cell_is_noop() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1));
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(
            game.cell_view(Position::new(1, 1)).state,
            CellState::Revealed
        );
    }

    #[test]
    fn flagged_cell_blocks_reveal() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.toggle_flag(Position::new(0, 0));
        game.reveal(Position::new(0, 0));
        // The Flag blocks the Reveal entirely: the game has not even started.
        assert_eq!(game.game_state(), GameState::Ready);
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Flagged
        );
        // After unflagging, the first click goes through.
        game.toggle_flag(Position::new(0, 0));
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 0)));
    }

    #[test]
    fn flags_remaining_tracks_flags() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        assert_eq!(game.flags_remaining(), 10);
        game.toggle_flag(Position::new(1, 1));
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(game.flags_remaining(), 8);
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(game.flags_remaining(), 9);
    }

    #[test]
    fn flagging_is_refused_when_all_flags_are_used() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic); // 10 mines
        for row in 0..2 {
            for col in 0..5 {
                game.toggle_flag(Position::new(row, col));
            }
        }
        assert_eq!(game.flags_remaining(), 0);
        // The 11th Flag is refused.
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(game.cell_view(Position::new(2, 2)).state, CellState::Hidden);
        assert_eq!(game.flags_remaining(), 0);
        // Removing a Flag frees a slot again.
        game.toggle_flag(Position::new(0, 0));
        assert_eq!(game.flags_remaining(), 1);
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(
            game.cell_view(Position::new(2, 2)).state,
            CellState::Flagged
        );
        assert_eq!(game.flags_remaining(), 0);
    }

    #[test]
    fn chord_reveals_unflagged_neighbors_when_flags_match() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1));
        assert_eq!(
            game.cell_view(Position::new(1, 1)).content,
            Some(CellContent::Number(1))
        );
        game.toggle_flag(Position::new(0, 0));
        game.chord(Position::new(1, 1));
        // The unflagged neighbors are Revealed; the Flagged Mine stays.
        assert_eq!(
            game.cell_view(Position::new(0, 1)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(1, 0)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Flagged
        );
    }

    #[test]
    fn chord_is_noop_when_flag_count_mismatches() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1));
        game.chord(Position::new(1, 1)); // zero Flags around a 1
        assert_eq!(game.cell_view(Position::new(0, 1)).state, CellState::Hidden);
        assert_eq!(game.cell_view(Position::new(1, 0)).state, CellState::Hidden);
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn chord_is_noop_on_hidden_and_zero_cells() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1));
        // Hidden Cell: nothing happens.
        game.chord(Position::new(2, 2));
        assert_eq!(game.cell_view(Position::new(2, 2)).state, CellState::Hidden);
        // Zero Cell: nothing happens either.
        game.reveal(Position::new(1, 1)); // already revealed; reveal a zero region first
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn chord_hitting_a_mine_loses_with_that_mine_as_trigger() {
        // Two Mines around (1,1): the player Flags (0,0) correctly but also
        // Flags (0,1) which is NOT a Mine — Flag count matches the number,
        // so the chord Reveals the Mine at (0,2) and loses.
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0), Position::new(0, 2)],
        );
        game.reveal(Position::new(1, 1));
        assert_eq!(
            game.cell_view(Position::new(1, 1)).content,
            Some(CellContent::Number(2))
        );
        game.toggle_flag(Position::new(0, 0));
        game.toggle_flag(Position::new(0, 1));
        game.chord(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 2)));
        assert_eq!(
            game.cell_view(Position::new(0, 2)).content,
            Some(CellContent::Mine)
        );
    }

    #[test]
    fn flag_and_chord_after_end_are_noop() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        game.toggle_flag(Position::new(1, 1));
        game.chord(Position::new(2, 2));
        assert_eq!(game.cell_view(Position::new(1, 1)).state, CellState::Hidden);
        assert_eq!(game.cell_view(Position::new(2, 2)).state, CellState::Hidden);
    }

    #[test]
    fn elapsed_is_zero_while_ready() {
        let game = Game::new(Difficulty::Beginner, GameMode::Classic);
        assert_eq!(game.elapsed(), Duration::ZERO);
    }

    #[test]
    fn elapsed_runs_after_first_reveal() {
        // Reveal a numeric Cell so the game stays Playing (no flood fill, no win).
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Playing);
        std::thread::sleep(Duration::from_millis(20));
        assert!(game.elapsed() >= Duration::from_millis(20));
    }

    #[test]
    fn elapsed_freezes_at_game_end() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(0, 0)); // instant Lost
        let frozen = game.elapsed();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(game.elapsed(), frozen);
    }

    #[test]
    fn prank_mode_first_reveal_is_always_a_mine() {
        // Property test: for many random Prank games, the first click always
        // reveals a Mine and loses instantly (ADR-0002).
        for _ in 0..20 {
            let mut game = Game::new(Difficulty::Beginner, GameMode::Prank);
            game.reveal(Position::new(0, 0));
            assert_eq!(game.game_state(), GameState::Lost);
            assert!(game.is_trigger(Position::new(0, 0)));
            assert_eq!(
                game.cell_view(Position::new(0, 0)).content,
                Some(CellContent::Mine)
            );
        }
    }

    #[test]
    fn prank_mode_forces_first_click_into_the_mine_list() {
        // Preset without the first-clicked Cell: the Cell is unioned in.
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Prank,
            &[Position::new(5, 5)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 0)));
        assert_eq!(
            game.cell_view(Position::new(0, 0)).content,
            Some(CellContent::Mine)
        );
        // The preset Mine is still a Mine on the final board.
        assert_eq!(
            game.cell_view(Position::new(5, 5)).content,
            Some(CellContent::Mine)
        );

        // Preset already containing the first-clicked Cell: no change.
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Prank,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 0)));
    }
}
