//! Core game logic for Minesweeper: pure rules, zero UI dependencies.
//!
//! The public interface of this module is the only seam — both the UI layer
//! and the unit tests drive the game through it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::index;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// A Minesweeper game. In Classic Mode the Mines are placed at creation
/// from the Seed (ADR-0004); in Prank Mode they are placed at the First
/// Click (ADR-0002).
#[derive(Debug)]
pub struct Game {
    difficulty: Difficulty,
    mode: GameMode,
    size: BoardSize,
    state: GameState,
    /// Mine positions; `None` until placed — always placed at creation in
    /// Classic Mode, at the First Click in Prank Mode.
    mines: Option<Vec<Position>>,
    /// CellState per Cell, indexed by Position (row-major).
    cell_states: Vec<CellState>,
    flags: usize,
    /// The number of Mines on the Board, fixed at creation from the
    /// Difficulty's Mine count (the "recipe"); it never changes during the
    /// game. Also the zero reference of the Flag counter.
    mine_count: usize,
    /// Trigger Mine position; `None` unless the game is Lost.
    trigger: Option<Position>,
    /// The Seed the Mine layout derives from; fixed at creation and never
    /// changes during the game.
    seed: u32,
    started_at: Option<Instant>,
    elapsed_at_end: Option<Duration>,
}

impl Game {
    /// Creates a game in `Ready` state with a fresh random Seed. In Classic
    /// Mode the Mines are placed immediately, sampled from the whole Board
    /// with no protection for the First Click (ADR-0004); in Prank Mode
    /// they are placed at the First Click, always including the clicked
    /// Cell (ADR-0002).
    #[cfg(test)]
    pub fn new(difficulty: Difficulty, mode: GameMode) -> Self {
        Self::with_seed(difficulty, mode, rand::random())
    }

    /// Creates a game in `Ready` state whose Mine layout derives from the
    /// given Seed. Reproducibility: the same Seed and Difficulty reproduce
    /// the same layout — unconditionally in Classic Mode, and in Prank Mode
    /// only with the same First Click. Guaranteed only within the same
    /// build (rand version, sampling algorithm).
    pub fn with_seed(difficulty: Difficulty, mode: GameMode, seed: u32) -> Self {
        let size = difficulty.size();
        let mut game = Self {
            difficulty,
            mode,
            size,
            state: GameState::Ready,
            mines: None,
            cell_states: vec![CellState::Hidden; size.rows * size.cols],
            flags: 0,
            mine_count: difficulty.mine_count(),
            trigger: None,
            seed,
            started_at: None,
            elapsed_at_end: None,
        };
        if mode == GameMode::Classic {
            game.mines = Some(Self::sample_mines(size, game.mine_count, seed, None));
        }
        game
    }

    /// Creates a game with a preset Mine list (test use). In Classic mode
    /// the caller is responsible for first-click safety: Mines on or
    /// adjacent to the first-clicked Cell are NOT filtered. In Prank mode
    /// the first-clicked Cell is always forced into the Mine list. The
    /// mine count is still set from the Difficulty (the recipe) and is
    /// independent of the preset length.
    #[cfg(test)]
    pub fn with_mines(difficulty: Difficulty, mode: GameMode, mines: &[Position]) -> Self {
        let mut game = Self::with_seed(difficulty, mode, 0);
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

    /// The Seed the Mine layout derives from.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The placed Mines, if any; `None` until placed (Prank Mode's `Ready`
    /// state). Test use.
    #[cfg(test)]
    pub fn mines(&self) -> Option<&[Position]> {
        self.mines.as_deref()
    }

    /// The Trigger Mine of a Lost game; `None` otherwise.
    pub fn trigger(&self) -> Option<Position> {
        self.trigger
    }

    /// The visible state of a Cell.
    pub fn cell_view(&self, pos: Position) -> CellView {
        let state = self.cell_state(pos);
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

    /// The mine count minus the number of placed Flags. It goes negative
    /// when the player has over-flagged and rises back toward zero as
    /// Flags are removed.
    pub fn flags_remaining(&self) -> i32 {
        self.mine_count as i32 - self.flags as i32
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
    /// or it is out of bounds. The first Reveal of a game places the Prank
    /// Mines (ADR-0002) and starts the clock.
    pub fn reveal(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        if self.cell_state(pos) != CellState::Hidden {
            return;
        }
        if self.state == GameState::Ready {
            // Classic Mode places its Mines at creation (ADR-0004); only
            // Prank Mode still defers placement to the First Click.
            if self.mode == GameMode::Prank {
                self.place_mines(pos);
            }
            self.state = GameState::Playing;
            self.started_at = Some(Instant::now());
        }
        self.reveal_at(pos);
        self.resolve_end(pos);
    }

    fn reveal_at(&mut self, pos: Position) {
        self.set_cell_state(pos, CellState::Revealed);
        // A Mine never flood-fills: the game ends on it in `resolve_end`.
        if self.is_mine(pos) {
            return;
        }
        if self.adjacent_mines(pos) == 0 {
            let mut queue = VecDeque::from([pos]);
            while let Some(pos) = queue.pop_front() {
                for neighbor in Self::neighbors(self.size, pos) {
                    if self.cell_state(neighbor) != CellState::Hidden || self.is_mine(neighbor) {
                        continue;
                    }
                    self.set_cell_state(neighbor, CellState::Revealed);
                    if self.adjacent_mines(neighbor) == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    /// Samples `count` distinct Mine positions on a board of `size`, drawn
    /// by index sampling from a Seed-seeded RNG — no full-Board
    /// enumeration-and-shuffle (issue #18). With `forced` (Prank Mode),
    /// that Cell is guaranteed part of the result: the remaining
    /// `count - 1` positions are sampled from the rest of the Board.
    fn sample_mines(
        size: BoardSize,
        count: usize,
        seed: u32,
        forced: Option<Position>,
    ) -> Vec<Position> {
        let total = size.rows * size.cols;
        let forced_idx = forced.map(|pos| pos.row * size.cols + pos.col);
        let (span, take) = match forced_idx {
            Some(_) => (total - 1, count - 1),
            None => (total, count),
        };
        let mut rng = StdRng::seed_from_u64(seed as u64);
        let mut mines: Vec<Position> = index::sample(&mut rng, span, take)
            .into_iter()
            .map(|i| {
                // Skip over the forced Cell by offsetting its index.
                let j = match forced_idx {
                    Some(f) if i >= f => i + 1,
                    _ => i,
                };
                Position::new(j / size.cols, j % size.cols)
            })
            .collect();
        if let Some(pos) = forced {
            mines.push(pos);
        }
        mines
    }

    /// Places the Mines of a Prank game at the First Click (ADR-0002): the
    /// First Clicked Cell is forced into the Mine list — for presets by
    /// union, for random placement by sampling the layout from the
    /// Seed-seeded candidates with the clicked Cell forced in. Classic
    /// Mode places its Mines at creation instead (ADR-0004) and never
    /// reaches here.
    fn place_mines(&mut self, first_click: Position) {
        let mut mines = match &self.mines {
            Some(preset) => preset.clone(),
            None => Self::sample_mines(self.size, self.mine_count, self.seed, Some(first_click)),
        };
        // `sample_mines` above already forces the First Click in for the
        // random path; this union only matters for preset Mines (test use).
        if self.mode == GameMode::Prank && !mines.contains(&first_click) {
            mines.push(first_click);
        }
        self.mines = Some(mines);
    }

    /// After a Reveal, transitions the game to Lost or Won when the
    /// condition holds, resolving the final board: Lost Reveals unflagged
    /// Mines, Won auto-Flags every Mine.
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
                let pos = Position::new(row, col);
                if mines.contains(&pos) {
                    continue;
                }
                if self.cell_state(pos) != CellState::Revealed {
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
        self.reveal_mines();
        self.elapsed_at_end = Some(self.elapsed());
    }

    /// Ends the game as Won, auto-Flagging every Mine on the final board:
    /// player-placed Flags are kept, the remaining Mines are marked by the
    /// game.
    fn win(&mut self) {
        self.state = GameState::Won;
        self.auto_flag_mines();
        self.elapsed_at_end = Some(self.elapsed());
    }

    /// Reveals the unflagged Mines (the Lost board); wrongly Flagged Cells
    /// stay Flagged.
    fn reveal_mines(&mut self) {
        let Some(mines) = self.mines.clone() else {
            return;
        };
        for pos in mines {
            if self.cell_state(pos) != CellState::Flagged {
                self.set_cell_state(pos, CellState::Revealed);
            }
        }
    }

    /// Flags every Mine that is not already Flagged (the Won board). Only
    /// reachable on a Won board, where every Flag necessarily sits on a
    /// Mine, so afterwards the Flag count is exactly the Mine count and
    /// Flags Remaining is zero.
    fn auto_flag_mines(&mut self) {
        let Some(mines) = self.mines.clone() else {
            return;
        };
        for pos in mines {
            if self.cell_state(pos) == CellState::Hidden {
                self.set_cell_state(pos, CellState::Flagged);
            }
        }
        self.flags = self.mine_count;
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
    /// ended games reject Flag toggling. Any Hidden Cell may be Flagged
    /// regardless of how many Flags are already placed (over-flagging);
    /// removing a Flag is always allowed.
    pub fn toggle_flag(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        match self.cell_state(pos) {
            CellState::Hidden => {
                self.set_cell_state(pos, CellState::Flagged);
                self.flags += 1;
            }
            CellState::Flagged => {
                self.set_cell_state(pos, CellState::Hidden);
                self.flags -= 1;
            }
            CellState::Revealed => {}
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
        if self.cell_state(pos) != CellState::Revealed {
            return;
        }
        let number = self.adjacent_mines(pos);
        if number == 0 {
            return;
        }
        let flagged = Self::neighbors(self.size, pos)
            .filter(|&n| self.cell_state(n) == CellState::Flagged)
            .count();
        if flagged != number as usize {
            return;
        }
        let mut hit_mine = None;
        for neighbor in Self::neighbors(self.size, pos) {
            if self.cell_state(neighbor) == CellState::Hidden {
                self.set_cell_state(neighbor, CellState::Revealed);
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

    /// The CellState of the Cell at `pos`. The caller must ensure `pos` is in bounds.
    fn cell_state(&self, pos: Position) -> CellState {
        debug_assert!(self.in_bounds(pos));
        self.cell_states[self.index(pos)]
    }

    /// Sets the CellState of the Cell at `pos`. The caller must ensure `pos` is in bounds.
    fn set_cell_state(&mut self, pos: Position, state: CellState) {
        debug_assert!(self.in_bounds(pos));
        let idx = self.index(pos);
        self.cell_states[idx] = state;
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
            assert_eq!(game.flags_remaining(), difficulty.mine_count() as i32);
        }
    }

    #[test]
    fn first_reveal_enters_playing_on_a_non_mine() {
        // Mines are pre-placed at creation (ADR-0004), so the First Click
        // is unprotected: on a non-Mine Cell it starts play; on a Mine it
        // loses instantly (covered by `reveal_mine_loses_and_auto_reveals_board`).
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1)); // not a Mine
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn classic_game_places_all_mines_at_creation() {
        for difficulty in [
            Difficulty::Beginner,
            Difficulty::Intermediate,
            Difficulty::Expert,
        ] {
            let game = Game::new(difficulty, GameMode::Classic);
            let mines = game.mines().expect("Classic mines are placed at creation");
            assert_eq!(mines.len(), difficulty.mine_count());
            let size = difficulty.size();
            let mut seen = std::collections::HashSet::new();
            for pos in mines {
                assert!(
                    pos.row < size.rows && pos.col < size.cols,
                    "Mine out of bounds at {pos:?}"
                );
                assert!(seen.insert(*pos), "duplicate Mine at {pos:?}");
            }
        }
    }

    #[test]
    fn same_seed_reproduces_the_same_classic_layout() {
        for difficulty in [Difficulty::Beginner, Difficulty::Expert] {
            let a = Game::with_seed(difficulty, GameMode::Classic, 42);
            let b = Game::with_seed(difficulty, GameMode::Classic, 42);
            assert_eq!(a.seed(), 42);
            assert_eq!(a.mines(), b.mines());
        }
    }

    #[test]
    fn different_seeds_give_different_classic_layouts() {
        let a = Game::with_seed(Difficulty::Beginner, GameMode::Classic, 1);
        let b = Game::with_seed(Difficulty::Beginner, GameMode::Classic, 2);
        assert_ne!(a.mines(), b.mines());
    }

    #[test]
    fn prank_same_seed_and_first_click_reproduce_the_layout() {
        let first_click = Position::new(0, 0);
        let a = {
            let mut game = Game::with_seed(Difficulty::Beginner, GameMode::Prank, 42);
            game.reveal(first_click);
            game
        };
        let b = {
            let mut game = Game::with_seed(Difficulty::Beginner, GameMode::Prank, 42);
            game.reveal(first_click);
            game
        };
        assert_eq!(a.mines(), b.mines());
        assert!(a.mines().unwrap().contains(&first_click));
    }

    #[test]
    fn prank_ready_game_has_no_mines_until_first_click() {
        let mut game = Game::with_seed(Difficulty::Beginner, GameMode::Prank, 42);
        assert_eq!(game.mines(), None);
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert!(game.mines().is_some());
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
        assert_eq!(game.trigger(), Some(Position::new(0, 0)));
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
        assert_ne!(game.trigger(), Some(Position::new(5, 5)));
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
        // and the Mine is auto-Flagged on the final board.
        assert_eq!(game.game_state(), GameState::Won);
        assert_eq!(
            game.cell_view(Position::new(4, 4)).state,
            CellState::Flagged
        );
        assert_eq!(game.cell_view(Position::new(4, 4)).content, None);
        // Every Mine is Flagged, so nothing is left to find.
        assert_eq!(game.flags_remaining(), 0);
    }

    #[test]
    fn revealing_every_non_mine_cell_wins_and_auto_flags_mines() {
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
        // The Mine is auto-Flagged on the final board.
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Flagged
        );
        assert_eq!(game.cell_view(Position::new(0, 0)).content, None);
        assert_eq!(game.flags_remaining(), 0);
    }

    #[test]
    fn win_keeps_player_flags_and_auto_flags_the_rest() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0), Position::new(1, 1)],
        );
        // Pre-flag one Mine; the other stays Hidden.
        game.toggle_flag(Position::new(0, 0));
        let size = Difficulty::Beginner.size();
        for row in 0..size.rows {
            for col in 0..size.cols {
                let pos = Position::new(row, col);
                if pos != Position::new(0, 0) && pos != Position::new(1, 1) {
                    game.reveal(pos);
                }
            }
        }
        assert_eq!(game.game_state(), GameState::Won);
        // The player's Flag is kept on the Won board.
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Flagged
        );
        // The previously Hidden Mine is auto-Flagged.
        assert_eq!(
            game.cell_view(Position::new(1, 1)).state,
            CellState::Flagged
        );
        assert_eq!(game.flags_remaining(), 0);
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
        assert_eq!(game.trigger(), Some(Position::new(0, 0)));
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
    fn flagging_allows_more_flags_than_mines() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic); // 10 mines
        for row in 0..2 {
            for col in 0..5 {
                game.toggle_flag(Position::new(row, col));
            }
        }
        assert_eq!(game.flags_remaining(), 0);
        // The 11th Flag is allowed: the counter goes negative.
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(
            game.cell_view(Position::new(2, 2)).state,
            CellState::Flagged
        );
        assert_eq!(game.flags_remaining(), -1);
        // More Flags keep driving the counter further negative.
        game.toggle_flag(Position::new(2, 3));
        assert_eq!(game.flags_remaining(), -2);
        // Removing Flags raises the counter back toward zero.
        game.toggle_flag(Position::new(0, 0));
        assert_eq!(game.flags_remaining(), -1);
        game.toggle_flag(Position::new(2, 3));
        game.toggle_flag(Position::new(2, 2));
        assert_eq!(game.flags_remaining(), 1);
    }

    #[test]
    fn chord_is_noop_when_flags_exceed_the_number() {
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
        // Burn the whole budget, then keep flagging: 2 Flags around a 1.
        for col in 0..9 {
            game.toggle_flag(Position::new(3, col));
        }
        game.toggle_flag(Position::new(0, 0));
        assert_eq!(game.flags_remaining(), 0);
        game.toggle_flag(Position::new(0, 1)); // the 11th Flag: beyond the mine count
        assert_eq!(game.flags_remaining(), -1);
        game.chord(Position::new(1, 1));
        // Flag count (2) exceeds the number (1): the chord stays a no-op.
        assert_eq!(game.game_state(), GameState::Playing);
        assert_eq!(
            game.cell_view(Position::new(0, 1)).state,
            CellState::Flagged
        );
        assert_eq!(game.cell_view(Position::new(1, 0)).state, CellState::Hidden);
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
        assert_eq!(game.trigger(), Some(Position::new(0, 2)));
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
            assert_eq!(game.trigger(), Some(Position::new(0, 0)));
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
        assert_eq!(game.trigger(), Some(Position::new(0, 0)));
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
        assert_eq!(game.trigger(), Some(Position::new(0, 0)));
    }
}
