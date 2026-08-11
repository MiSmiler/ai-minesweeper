//! Core game logic for Minesweeper: pure rules, zero UI dependencies.
//!
//! The public interface of this module is the only seam — both the UI layer
//! and the unit tests drive the game through it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;

/// One of the three classic difficulty presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Expert,
}

impl Difficulty {
    /// Board size as (columns, rows) in Cells.
    pub fn size(self) -> (usize, usize) {
        match self {
            Difficulty::Beginner => (9, 9),
            Difficulty::Intermediate => (16, 16),
            Difficulty::Expert => (30, 16),
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
    width: usize,
    height: usize,
    state: GameState,
    /// Mine positions; `None` until the first Reveal places them.
    mines: Option<Vec<Position>>,
    cells: Vec<CellState>,
    flags: usize,
    trigger: Option<Position>,
    started_at: Option<Instant>,
    ended_elapsed: Option<Duration>,
}

impl Game {
    /// Creates a game in `Ready` state. Mines are placed randomly on the
    /// first Reveal, never on or adjacent to the first-clicked Cell.
    pub fn new(difficulty: Difficulty) -> Self {
        let (width, height) = difficulty.size();
        Self {
            difficulty,
            width,
            height,
            state: GameState::Ready,
            mines: None,
            cells: vec![CellState::Hidden; width * height],
            flags: 0,
            trigger: None,
            started_at: None,
            ended_elapsed: None,
        }
    }

    /// Creates a game with a preset Mine list (test use). The caller is
    /// responsible for first-click safety: Mines on or adjacent to the
    /// first-clicked Cell are NOT filtered.
    #[cfg(test)]
    pub fn with_mines(difficulty: Difficulty, mines: &[Position]) -> Self {
        let mut game = Self::new(difficulty);
        game.mines = Some(mines.to_vec());
        game
    }

    /// The current game state.
    pub fn game_state(&self) -> GameState {
        self.state
    }

    /// The board size as (columns, rows).
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
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
    pub fn cell_state(&self, pos: Position) -> CellView {
        let idx = self.index(pos);
        let state = self.cells[idx];
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

    /// Total Mines minus Flags placed. Never below zero: Flagging is refused
    /// once it reaches zero (Flags cannot exceed the Mine count).
    pub fn mines_remaining(&self) -> usize {
        self.difficulty.mine_count() - self.flags
    }

    /// Whether the game has ended (Won or Lost).
    fn ended(&self) -> bool {
        matches!(self.state, GameState::Won | GameState::Lost)
    }

    /// Whether the game is running and the Cell lies on the board.
    fn is_active(&self, pos: Position) -> bool {
        !self.ended() && self.in_bounds(pos)
    }

    /// Whether the Cell lies on the board.
    fn in_bounds(&self, pos: Position) -> bool {
        pos.row < self.height && pos.col < self.width
    }

    /// Reveals a Cell. No-op when the game has ended, the Cell is Flagged,
    /// or it is out of bounds. The first Reveal of a game places the Mines
    /// (ADR-0001) and starts the clock.
    pub fn reveal(&mut self, pos: Position) {
        if !self.is_active(pos) {
            return;
        }
        let idx = self.index(pos);
        if self.cells[idx] != CellState::Hidden {
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
        let idx = self.index(pos);
        self.cells[idx] = CellState::Revealed;
        if self.adjacent_mines(pos) == 0 {
            let mut queue = VecDeque::from([pos]);
            while let Some(cell) = queue.pop_front() {
                for neighbor in Self::neighbors(self.width, self.height, cell) {
                    let nidx = self.index(neighbor);
                    if self.cells[nidx] != CellState::Hidden || self.is_mine(neighbor) {
                        continue;
                    }
                    self.cells[nidx] = CellState::Revealed;
                    if self.adjacent_mines(neighbor) == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    /// Places the Mines for a new game, excluding the first-clicked Cell and
    /// its 8 neighbors (ADR-0001). A preset list from `with_mines` is trusted
    /// as-is; random placement samples from the remaining Cells.
    fn place_mines(&mut self, first: Position) {
        let mines = match &self.mines {
            Some(preset) => preset.clone(),
            None => {
                let mut candidates: Vec<Position> = Vec::new();
                for row in 0..self.height {
                    for col in 0..self.width {
                        let cell = Position::new(row, col);
                        let excluded = cell == first
                            || (row.abs_diff(first.row) <= 1 && col.abs_diff(first.col) <= 1);
                        if !excluded {
                            candidates.push(cell);
                        }
                    }
                }
                let mut rng = rand::rng();
                candidates.shuffle(&mut rng);
                candidates.truncate(self.difficulty.mine_count());
                candidates
            }
        };
        self.mines = Some(mines);
    }

    /// After a Reveal, transitions the game to Lost or Won when the
    /// condition holds, auto-Revealing the Mines on the final board.
    fn resolve_end(&mut self, pos: Position) {
        if self.is_mine(pos) {
            self.lose(pos);
        } else {
            self.check_win();
        }
    }

    /// Ends the game as Lost on the given Trigger Mine, auto-Revealing all
    /// unflagged Mines; wrongly Flagged Cells stay Flagged.
    fn lose(&mut self, pos: Position) {
        self.state = GameState::Lost;
        self.trigger = Some(pos);
        self.reveal_mines(false);
        self.ended_elapsed = Some(self.elapsed());
    }

    /// Ends the game as Won when every non-Mine Cell is Revealed,
    /// auto-Revealing all Mines on the final board.
    fn check_win(&mut self) {
        let all_safe_revealed = self.mines.as_ref().is_some_and(|mines| {
            (0..self.height).all(|row| {
                (0..self.width).all(|col| {
                    let pos = Position::new(row, col);
                    mines.contains(&pos) || self.cells[self.index(pos)] == CellState::Revealed
                })
            })
        });
        if all_safe_revealed {
            self.state = GameState::Won;
            self.reveal_mines(true);
            self.ended_elapsed = Some(self.elapsed());
        }
    }

    /// Reveals the Mines. When `include_flagged` is false, Flagged Mines
    /// keep their Flag (Lost board); when true, all Mines are Revealed (Won).
    fn reveal_mines(&mut self, include_flagged: bool) {
        if let Some(mines) = &self.mines {
            for &pos in mines {
                let idx = self.index(pos);
                if include_flagged || self.cells[idx] != CellState::Flagged {
                    self.cells[idx] = CellState::Revealed;
                }
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        match (self.started_at, self.ended_elapsed) {
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
        Self::neighbors(self.width, self.height, pos)
            .filter(|&n| self.is_mine(n))
            .count() as u8
    }

    fn neighbors(width: usize, height: usize, pos: Position) -> impl Iterator<Item = Position> {
        let (min_r, max_r) = (pos.row.saturating_sub(1), (pos.row + 1).min(height - 1));
        let (min_c, max_c) = (pos.col.saturating_sub(1), (pos.col + 1).min(width - 1));
        (min_r..=max_r).flat_map(move |r| (min_c..=max_c).map(move |c| Position::new(r, c)))
    }

    /// Toggles a Flag on a Hidden Cell. No-op otherwise: Revealed Cells and
    /// ended games reject Flag toggling. Flagging is refused once the Mine
    /// Counter has reached zero (Flags cannot exceed the Mine count);
    /// removing a Flag is always allowed.
    pub fn toggle_flag(&mut self, pos: Position) {
        if !self.is_active(pos) {
            return;
        }
        let idx = self.index(pos);
        match self.cells[idx] {
            CellState::Hidden if self.flags < self.difficulty.mine_count() => {
                self.cells[idx] = CellState::Flagged;
                self.flags += 1;
            }
            CellState::Flagged => {
                self.cells[idx] = CellState::Hidden;
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
        if !self.is_active(pos) {
            return;
        }
        if self.cells[self.index(pos)] != CellState::Revealed {
            return;
        }
        let number = self.adjacent_mines(pos);
        if number == 0 {
            return;
        }
        let flagged = Self::neighbors(self.width, self.height, pos)
            .filter(|&n| self.cells[self.index(n)] == CellState::Flagged)
            .count();
        if flagged != number as usize {
            return;
        }
        let mut hit_mine = None;
        for neighbor in Self::neighbors(self.width, self.height, pos) {
            let nidx = self.index(neighbor);
            if self.cells[nidx] == CellState::Hidden {
                self.cells[nidx] = CellState::Revealed;
                if self.is_mine(neighbor) && hit_mine.is_none() {
                    hit_mine = Some(neighbor);
                }
            }
        }
        match hit_mine {
            Some(pos) => self.lose(pos),
            None => self.check_win(),
        }
    }

    fn index(&self, pos: Position) -> usize {
        pos.row * self.width + pos.col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_presets_have_classic_sizes_and_mine_counts() {
        assert_eq!(Difficulty::Beginner.size(), (9, 9));
        assert_eq!(Difficulty::Beginner.mine_count(), 10);
        assert_eq!(Difficulty::Intermediate.size(), (16, 16));
        assert_eq!(Difficulty::Intermediate.mine_count(), 40);
        assert_eq!(Difficulty::Expert.size(), (30, 16));
        assert_eq!(Difficulty::Expert.mine_count(), 99);
    }

    #[test]
    fn new_game_starts_ready_with_all_cells_hidden() {
        let game = Game::new(Difficulty::Beginner);
        assert_eq!(game.game_state(), GameState::Ready);
        let (width, height) = Difficulty::Beginner.size();
        for row in 0..height {
            for col in 0..width {
                let pos = Position::new(row, col);
                assert_eq!(game.cell_state(pos).state, CellState::Hidden);
                assert_eq!(game.cell_state(pos).content, None);
            }
        }
    }

    #[test]
    fn mines_remaining_equals_total_before_any_flag() {
        for difficulty in [
            Difficulty::Beginner,
            Difficulty::Intermediate,
            Difficulty::Expert,
        ] {
            let game = Game::new(difficulty);
            assert_eq!(game.mines_remaining(), difficulty.mine_count());
        }
    }

    #[test]
    fn first_reveal_enters_playing_and_is_always_safe() {
        // Property test: for many random games, the first click and its 8
        // neighbors are never Mines (ADR-0001).
        for _ in 0..20 {
            let mut game = Game::new(Difficulty::Beginner);
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
            &[Position::new(0, 0), Position::new(5, 5)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 0)));
        // The Trigger Mine is Revealed and shown as Mine.
        assert_eq!(
            game.cell_state(Position::new(0, 0)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(0, 0)).content,
            Some(CellContent::Mine)
        );
        // The other unflagged Mine is auto-Revealed too.
        assert_eq!(
            game.cell_state(Position::new(5, 5)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(5, 5)).content,
            Some(CellContent::Mine)
        );
        // The Trigger Mine is the only one flagged as trigger.
        assert!(!game.is_trigger(Position::new(5, 5)));
    }

    #[test]
    fn reveal_shows_neighbor_count() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(2, 2)]);
        game.reveal(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Playing);
        assert_eq!(
            game.cell_state(Position::new(1, 1)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(1, 1)).content,
            Some(CellContent::Number(1))
        );
    }

    #[test]
    fn zero_cell_flood_fills_until_numbered_boundary() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(4, 4)]);
        game.reveal(Position::new(0, 0));
        // The clicked Cell is a zero Cell.
        assert_eq!(
            game.cell_state(Position::new(0, 0)).content,
            Some(CellContent::Number(0))
        );
        // The numbered boundary of the flood fill is Revealed.
        assert_eq!(
            game.cell_state(Position::new(3, 3)).content,
            Some(CellContent::Number(1))
        );
        // A far corner in the zero region is Revealed.
        assert_eq!(
            game.cell_state(Position::new(8, 8)).content,
            Some(CellContent::Number(0))
        );
        // One lone Mine means the flood fill wins instantly: the game is Won
        // and the Mine is auto-Revealed on the final board.
        assert_eq!(game.game_state(), GameState::Won);
        assert_eq!(
            game.cell_state(Position::new(4, 4)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(4, 4)).content,
            Some(CellContent::Mine)
        );
    }

    #[test]
    fn revealing_every_non_mine_cell_wins_and_reveals_mines() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        let (width, height) = Difficulty::Beginner.size();
        for row in 0..height {
            for col in 0..width {
                if Position::new(row, col) != Position::new(0, 0) {
                    game.reveal(Position::new(row, col));
                }
            }
        }
        assert_eq!(game.game_state(), GameState::Won);
        // The Mine is auto-Revealed on the final board.
        assert_eq!(
            game.cell_state(Position::new(0, 0)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(0, 0)).content,
            Some(CellContent::Mine)
        );
    }

    #[test]
    fn ended_game_rejects_reveals() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        // Reveals after the end change nothing.
        game.reveal(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Lost);
        assert_eq!(
            game.cell_state(Position::new(1, 1)).state,
            CellState::Hidden
        );
    }

    #[test]
    fn reveal_out_of_bounds_is_noop() {
        let mut game = Game::new(Difficulty::Beginner);
        game.reveal(Position::new(99, 99));
        assert_eq!(game.game_state(), GameState::Ready);
    }

    #[test]
    fn flag_toggles_hidden_to_flagged_and_back() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(
            game.cell_state(Position::new(1, 1)).state,
            CellState::Flagged
        );
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(
            game.cell_state(Position::new(1, 1)).state,
            CellState::Hidden
        );
    }

    #[test]
    fn flag_on_revealed_cell_is_noop() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(1, 1));
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(
            game.cell_state(Position::new(1, 1)).state,
            CellState::Revealed
        );
    }

    #[test]
    fn flagged_cell_blocks_reveal() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.toggle_flag(Position::new(0, 0));
        game.reveal(Position::new(0, 0));
        // The Flag blocks the Reveal entirely: the game has not even started.
        assert_eq!(game.game_state(), GameState::Ready);
        assert_eq!(
            game.cell_state(Position::new(0, 0)).state,
            CellState::Flagged
        );
        // After unflagging, the first click goes through.
        game.toggle_flag(Position::new(0, 0));
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 0)));
    }

    #[test]
    fn mines_remaining_tracks_flags() {
        let mut game = Game::new(Difficulty::Beginner);
        assert_eq!(game.mines_remaining(), 10);
        game.toggle_flag(Position::new(1, 1));
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(game.mines_remaining(), 8);
        game.toggle_flag(Position::new(1, 1));
        assert_eq!(game.mines_remaining(), 9);
    }

    #[test]
    fn flagging_is_refused_when_all_flags_are_used() {
        let mut game = Game::new(Difficulty::Beginner); // 10 mines
        for row in 0..2 {
            for col in 0..5 {
                game.toggle_flag(Position::new(row, col));
            }
        }
        assert_eq!(game.mines_remaining(), 0);
        // The 11th Flag is refused.
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(
            game.cell_state(Position::new(2, 2)).state,
            CellState::Hidden
        );
        assert_eq!(game.mines_remaining(), 0);
        // Removing a Flag frees a slot again.
        game.toggle_flag(Position::new(0, 0));
        assert_eq!(game.mines_remaining(), 1);
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(
            game.cell_state(Position::new(2, 2)).state,
            CellState::Flagged
        );
        assert_eq!(game.mines_remaining(), 0);
    }

    #[test]
    fn chord_sweeps_unflagged_neighbors_when_flags_match() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(1, 1));
        assert_eq!(
            game.cell_state(Position::new(1, 1)).content,
            Some(CellContent::Number(1))
        );
        game.toggle_flag(Position::new(0, 0));
        game.chord(Position::new(1, 1));
        // The unflagged neighbors are Revealed; the Flagged Mine stays.
        assert_eq!(
            game.cell_state(Position::new(0, 1)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(1, 0)).state,
            CellState::Revealed
        );
        assert_eq!(
            game.cell_state(Position::new(0, 0)).state,
            CellState::Flagged
        );
    }

    #[test]
    fn chord_is_noop_when_flag_count_mismatches() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(1, 1));
        game.chord(Position::new(1, 1)); // zero Flags around a 1
        assert_eq!(
            game.cell_state(Position::new(0, 1)).state,
            CellState::Hidden
        );
        assert_eq!(
            game.cell_state(Position::new(1, 0)).state,
            CellState::Hidden
        );
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn chord_is_noop_on_hidden_and_zero_cells() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(1, 1));
        // Hidden Cell: nothing happens.
        game.chord(Position::new(2, 2));
        assert_eq!(
            game.cell_state(Position::new(2, 2)).state,
            CellState::Hidden
        );
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
            &[Position::new(0, 0), Position::new(0, 2)],
        );
        game.reveal(Position::new(1, 1));
        assert_eq!(
            game.cell_state(Position::new(1, 1)).content,
            Some(CellContent::Number(2))
        );
        game.toggle_flag(Position::new(0, 0));
        game.toggle_flag(Position::new(0, 1));
        game.chord(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.is_trigger(Position::new(0, 2)));
        assert_eq!(
            game.cell_state(Position::new(0, 2)).content,
            Some(CellContent::Mine)
        );
    }

    #[test]
    fn flag_and_chord_after_end_are_noop() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        game.toggle_flag(Position::new(1, 1));
        game.chord(Position::new(2, 2));
        assert_eq!(
            game.cell_state(Position::new(1, 1)).state,
            CellState::Hidden
        );
        assert_eq!(
            game.cell_state(Position::new(2, 2)).state,
            CellState::Hidden
        );
    }

    #[test]
    fn elapsed_is_zero_while_ready() {
        let game = Game::new(Difficulty::Beginner);
        assert_eq!(game.elapsed(), Duration::ZERO);
    }

    #[test]
    fn elapsed_runs_after_first_reveal() {
        // Reveal a numeric Cell so the game stays Playing (no flood fill, no win).
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(1, 1));
        assert_eq!(game.game_state(), GameState::Playing);
        std::thread::sleep(Duration::from_millis(20));
        assert!(game.elapsed() >= Duration::from_millis(20));
    }

    #[test]
    fn elapsed_freezes_at_game_end() {
        let mut game = Game::with_mines(Difficulty::Beginner, &[Position::new(0, 0)]);
        game.reveal(Position::new(0, 0)); // instant Lost
        let frozen = game.elapsed();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(game.elapsed(), frozen);
    }
}
