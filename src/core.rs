//! Core game logic for Minesweeper: pure rules, with no UI, framework,
//! network, or serde dependencies — freely testable through its public seam.
//! Domain enums expose their canonical names via `as_str()`, which the wire
//! and format adapters reuse instead of re-deriving a mapping per adapter.
//!
//! The public interface of this module is the only seam — both the UI layer
//! and the unit tests drive the game through it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::index;

use tracing::{debug, info};

/// The value fixed when a Game is created that determines the Mine layout;
/// a backend detail the player never sees, and reproducibility is
/// guaranteed only within the same build.
pub type Seed = u32;

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

    /// The canonical wire/format name: `beginner` / `intermediate` / `expert`.
    pub fn as_str(self) -> &'static str {
        match self {
            Difficulty::Beginner => "beginner",
            Difficulty::Intermediate => "intermediate",
            Difficulty::Expert => "expert",
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

impl CellState {
    /// The canonical wire name: `hidden` / `flagged` / `revealed`.
    pub fn as_str(self) -> &'static str {
        match self {
            CellState::Hidden => "hidden",
            CellState::Flagged => "flagged",
            CellState::Revealed => "revealed",
        }
    }
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
    /// Standard rules. A random game defers Mine placement to the First
    /// Click and regenerates the Seed until the clicked Cell's 3x3 is
    /// Mine-free, so the First Click is safe; a pinned `--seed` places the
    /// Mines at creation and reproduces the layout exactly, leaving the
    /// First Click unprotected (ADR-0004, amended by ADR-0009).
    Classic,
    /// Prank: the First Click is always a Mine, losing the game instantly.
    Prank,
}

impl GameMode {
    /// The canonical wire name: `classic` / `prank`.
    pub fn as_str(self) -> &'static str {
        match self {
            GameMode::Classic => "classic",
            GameMode::Prank => "prank",
        }
    }
}

/// The state of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    /// Board exists. A pinned Classic `--seed` has placed its Mines; a
    /// random Classic game or a Prank game has not yet.
    Ready,
    /// Mines placed, game in progress.
    Playing,
    /// All non-Mine Cells Revealed.
    Won,
    /// A Mine was Revealed.
    Lost,
}

impl GameState {
    /// The canonical wire/format name: `ready` / `playing` / `won` / `lost`.
    pub fn as_str(self) -> &'static str {
        match self {
            GameState::Ready => "ready",
            GameState::Playing => "playing",
            GameState::Won => "won",
            GameState::Lost => "lost",
        }
    }
}

/// A Minesweeper game. In Classic Mode a pinned `--seed` places the Mines at
/// creation (ADR-0004) and a random game defers them to the First Click
/// (ADR-0009); in Prank Mode they are placed at the First Click (ADR-0002).
#[derive(Debug)]
pub struct Game {
    difficulty: Difficulty,
    mode: GameMode,
    size: BoardSize,
    state: GameState,
    /// Mine positions; `None` until placed — placed at creation for a pinned
    /// Classic `--seed`, otherwise at the First Click.
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
    /// The Seed the Mine layout derives from. Fixed at creation for a pinned
    /// `--seed`; a random game's provisional candidate until the First Click
    /// commits it.
    seed: Seed,
    /// Whether the First Click must be safe: a random Classic game regenerates
    /// its Seed until the clicked Cell's 3x3 is Mine-free; a pinned `--seed`
    /// reproduces the layout exactly and leaves the First Click unprotected.
    /// Always false in Prank Mode (its First Click is a Mine by design).
    first_click_safe: bool,
    started_at: Option<Instant>,
    elapsed_at_end: Option<Duration>,
}

impl Game {
    /// Creates a game in `Ready` state with a fresh random Seed. A random
    /// Classic game defers Mine placement and regenerates the Seed until the
    /// First Click is safe (ADR-0009); a Prank game places its Mines at the
    /// First Click (ADR-0002). The Seed is a provisional candidate until the
    /// First Click commits it.
    pub fn new(difficulty: Difficulty, mode: GameMode) -> Self {
        Self::build(difficulty, mode, rand::random(), true)
    }

    /// Creates a game in `Ready` state whose Mine layout derives from the
    /// given Seed — a pinned `--seed`: the same Seed and Difficulty reproduce
    /// the same layout. In Classic Mode the Mines are placed at creation and
    /// the First Click is unprotected (reproduction mode, ADR-0004); in Prank
    /// Mode they are placed at the First Click (ADR-0002). Guaranteed only
    /// within the same build (rand version, sampling algorithm).
    pub fn with_seed(difficulty: Difficulty, mode: GameMode, seed: Seed) -> Self {
        Self::build(difficulty, mode, seed, false)
    }

    /// Builds a game. `first_click_safe` is honored only for Classic Mode: a
    /// safe game defers Mine placement (and regenerates the Seed until the
    /// First Click's 3x3 is Mine-free); an unsafe (pinned) game places the
    /// full Mine set at creation, reproducing the layout exactly.
    fn build(difficulty: Difficulty, mode: GameMode, seed: Seed, first_click_safe: bool) -> Self {
        let size = difficulty.size();
        let safe = first_click_safe && mode == GameMode::Classic;
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
            first_click_safe: safe,
            started_at: None,
            elapsed_at_end: None,
        };
        if mode == GameMode::Classic && !safe {
            game.commit_mines(
                Self::sample_mines(size, game.mine_count, seed, None),
                "at game creation",
            );
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

    /// The current candidate Seed (provisional until committed). Internal
    /// logging only; not an authoritative value. A random game always holds a
    /// candidate, so the "committed vs provisional" axis is boundary-visible
    /// (see `committed_seed`), never a `None` on the field.
    pub(crate) fn seed(&self) -> Seed {
        self.seed
    }

    /// The committed replay Seed, or `None` while the Mine layout isn't fixed
    /// yet. `Some` for a pinned Classic `--seed` from creation (ADR-0004);
    /// otherwise only once the First Click commits it (random Classic,
    /// ADR-0009; Prank, ADR-0002). It is committed exactly when a Mine list is
    /// set — the write-side seam is `commit_mines`. The internal field stays a
    /// plain `Seed` because the candidate is always present — the `Option`
    /// lives here, at this boundary, where the provisional candidate is never
    /// exposed.
    // `committed_seed` is the public read seam (ADR-0009). On `main` its only
    // caller today is the AI boundary, which hasn't landed yet, so the
    // non-test build would otherwise flag it as dead. It is the authoritative
    // read side and must stay.
    #[allow(dead_code)]
    pub fn committed_seed(&self) -> Option<Seed> {
        self.mines.is_some().then_some(self.seed)
    }

    /// Fixes the Mine layout, committing the Seed: the moment a Mine list
    /// becomes `Some` is exactly when the Seed is committed, so this is the
    /// single write-side seam for that event (read it back with
    /// `committed_seed`, and it logs the commit at `info`). A random game
    /// holds a candidate Seed until this runs, which is why the field stays a
    /// plain `Seed` (ADR-0009).
    fn commit_mines(&mut self, mines: Vec<Position>, at: &str) {
        self.mines = Some(mines);
        info!(
            seed = self.seed(),
            difficulty = self.difficulty.as_str(),
            "seed committed {at}"
        );
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
    /// Mines (ADR-0002) and starts the Timer.
    pub fn reveal(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        if self.cell_state(pos) != CellState::Hidden {
            return;
        }
        if self.state == GameState::Ready {
            // A pinned Classic `--seed` has its Mines placed at creation
            // (ADR-0004); a random Classic game defers them and regenerates
            // the Seed until the First Click is safe (ADR-0009); Prank Mode
            // places them at the First Click, forcing the clicked Cell in.
            match self.mode {
                GameMode::Prank => self.place_mines(pos),
                GameMode::Classic if self.first_click_safe => self.ensure_safe_first_click(pos),
                GameMode::Classic => {}
            }
            self.state = GameState::Playing;
            self.started_at = Some(Instant::now());
        }
        self.reveal_cell(pos);
        self.resolve_end(pos);
    }

    /// Reveals a single Cell and applies the cascade rule: a zero Cell (no
    /// adjacent Mines) also Reveals its surrounding non-Mine Cells,
    /// cascading through connected zero Cells. Shared by `reveal` (a
    /// click) and `chord` (each neighbor it Reveals is subject to the same
    /// rule). A Mine never cascades: the game ends on it in `resolve_end`.
    fn reveal_cell(&mut self, pos: Position) {
        self.set_cell_state(pos, CellState::Revealed);
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
        seed: Seed,
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
                let adjusted = match forced_idx {
                    Some(f) if i >= f => i + 1,
                    _ => i,
                };
                Position::new(adjusted / size.cols, adjusted % size.cols)
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
    /// Seed-seeded candidates with the clicked Cell forced in. Classic Mode
    /// never reaches here: a pinned `--seed` places its Mines at creation
    /// (ADR-0004), a random game defers to the First Click via
    /// `ensure_safe_first_click` (ADR-0009).
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
        self.commit_mines(mines, "at first click");
    }

    /// Makes a random Classic game's First Click safe (ADR-0009): the Seed is
    /// re-drawn until the clicked Cell's 3x3 is Mine-free, and the accepted
    /// Seed (with its layout) becomes the game's Mines.
    fn ensure_safe_first_click(&mut self, pos: Position) {
        let region: Vec<Position> = Self::neighbors(self.size, pos)
            .chain(std::iter::once(pos))
            .collect();
        loop {
            let mines = Self::sample_mines(self.size, self.mine_count, self.seed, None);
            if !region.iter().any(|p| mines.contains(p)) {
                self.commit_mines(mines, "at first click");
                return;
            }
            debug!(
                seed = self.seed,
                "seed candidate rejected; first click not safe"
            );
            self.seed = rand::random();
        }
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
    /// number, Reveals all unflagged neighbors — each subject to the same
    /// cascade rule as a click, so a zero Cell Revealed by the Chord
    /// cascades through connected zero Cells. No-op otherwise — including
    /// on Hidden Cells, zero Cells, and mismatched Flag counts.
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
                self.reveal_cell(neighbor);
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
    fn difficulty_canonical_names_match_the_wire() {
        assert_eq!(Difficulty::Beginner.as_str(), "beginner");
        assert_eq!(Difficulty::Intermediate.as_str(), "intermediate");
        assert_eq!(Difficulty::Expert.as_str(), "expert");
    }

    #[test]
    fn game_mode_canonical_names_match_the_wire() {
        assert_eq!(GameMode::Classic.as_str(), "classic");
        assert_eq!(GameMode::Prank.as_str(), "prank");
    }

    #[test]
    fn game_state_canonical_names_match_the_wire() {
        assert_eq!(GameState::Ready.as_str(), "ready");
        assert_eq!(GameState::Playing.as_str(), "playing");
        assert_eq!(GameState::Won.as_str(), "won");
        assert_eq!(GameState::Lost.as_str(), "lost");
    }

    #[test]
    fn cell_state_canonical_names_match_the_wire() {
        assert_eq!(CellState::Hidden.as_str(), "hidden");
        assert_eq!(CellState::Flagged.as_str(), "flagged");
        assert_eq!(CellState::Revealed.as_str(), "revealed");
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
        // A pinned `--seed` (here a preset) places its Mines at creation, so
        // the First Click outside them starts play; a Mine under the First
        // Click loses (covered by `reveal_mine_loses_and_auto_reveals_board`).
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1)); // not a Mine
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn pinned_classic_game_places_all_mines_at_creation() {
        for difficulty in [
            Difficulty::Beginner,
            Difficulty::Intermediate,
            Difficulty::Expert,
        ] {
            let game = Game::with_seed(difficulty, GameMode::Classic, 42);
            let mines = game
                .mines()
                .expect("pinned Classic mines are placed at creation");
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
    fn random_classic_ready_game_has_no_mines_until_first_click() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        assert_eq!(game.mines(), None);
        game.reveal(Position::new(0, 0));
        assert!(game.mines().is_some());
    }

    #[test]
    fn random_classic_first_click_is_safe_and_opens() {
        // A random Classic game guarantees the First Click is Mine-free: the
        // clicked Cell's 3x3 has no Mines, so it cascades as a zero Cell
        // (ADR-0009).
        for difficulty in [
            Difficulty::Beginner,
            Difficulty::Intermediate,
            Difficulty::Expert,
        ] {
            let size = difficulty.size();
            let first = Position::new(size.rows / 2, size.cols / 2);
            for _ in 0..8 {
                let mut game = Game::new(difficulty, GameMode::Classic);
                game.reveal(first);
                assert_ne!(game.game_state(), GameState::Lost);
                assert_eq!(
                    game.cell_view(first).content,
                    Some(CellContent::Number(0)),
                    "First Click {first:?} was not a safe zero Cell"
                );
            }
        }
    }

    #[test]
    fn random_game_accepted_seed_reproduces_the_safe_board() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        let first = Position::new(0, 0);
        game.reveal(first);
        let accepted = game.seed();
        let layout = game.mines().unwrap().to_vec();
        // Replay the accepted Seed as a pinned game: it reproduces the exact
        // layout, and the same First Click stays safe there (same Board).
        let mut replay = Game::with_seed(Difficulty::Beginner, GameMode::Classic, accepted);
        assert_eq!(replay.mines().unwrap(), &layout[..]);
        replay.reveal(first);
        assert_ne!(replay.game_state(), GameState::Lost);
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
    fn zero_cell_cascades_until_numbered_boundary() {
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
        // The numbered boundary of the cascade is Revealed.
        assert_eq!(
            game.cell_view(Position::new(3, 3)).content,
            Some(CellContent::Number(1))
        );
        // A far corner in the zero region is Revealed.
        assert_eq!(
            game.cell_view(Position::new(8, 8)).content,
            Some(CellContent::Number(0))
        );
        // One lone Mine means the cascade wins instantly: the game is Won
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
    fn chord_cascades_through_revealed_zero_cells() {
        // A solid wall of Mines across row 4 splits the Board in two. The
        // 3 at (3,1) sits against the wall; Flagging its three Mine
        // neighbors and Chording Reveals the zero Cells at (2,0),(2,1),
        // (2,2) — each subject to the same cascade rule as a click, so the
        // connected zero region of the top half cascades several levels
        // deep, while the bottom half stays untouched.
        let mut mines: Vec<Position> = (0..9).map(|c| Position::new(4, c)).collect();
        mines.push(Position::new(8, 8));
        let mut game = Game::with_mines(Difficulty::Beginner, GameMode::Classic, &mines);
        game.reveal(Position::new(3, 1));
        assert_eq!(
            game.cell_view(Position::new(3, 1)).content,
            Some(CellContent::Number(3))
        );
        for col in 0..3 {
            game.toggle_flag(Position::new(4, col));
        }
        game.chord(Position::new(3, 1));

        // The zero Cell the Chord revealed cascades like a click: level 1…
        assert_eq!(
            game.cell_view(Position::new(2, 0)).content,
            Some(CellContent::Number(0))
        );
        // …level 2…
        assert_eq!(
            game.cell_view(Position::new(1, 5)).content,
            Some(CellContent::Number(0))
        );
        // …and level 3, at the Board's top edge.
        assert_eq!(
            game.cell_view(Position::new(0, 5)).content,
            Some(CellContent::Number(0))
        );
        // The Mine wall bounds the cascade: the bottom half stays Hidden and
        // the game is still in progress.
        assert_eq!(game.cell_view(Position::new(5, 5)).state, CellState::Hidden);
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn chord_cascade_revealing_the_last_cell_wins() {
        // Two corner Mines: Chording the 2 at (1,1) Reveals the remaining
        // Mine-adjacent Cells directly, and the zero Cells it Reveals
        // cascade across every other non-Mine Cell — the last Reveal wins.
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0), Position::new(0, 1)],
        );
        game.reveal(Position::new(1, 1));
        game.toggle_flag(Position::new(0, 0));
        game.toggle_flag(Position::new(0, 1));
        game.chord(Position::new(1, 1));

        assert_eq!(game.game_state(), GameState::Won);
        // The Mines are auto-Flagged on the Won board.
        assert_eq!(
            game.cell_view(Position::new(0, 0)).state,
            CellState::Flagged
        );
        assert_eq!(
            game.cell_view(Position::new(0, 1)).state,
            CellState::Flagged
        );
        // A Cell deep in the cascaded region is Revealed.
        assert_eq!(
            game.cell_view(Position::new(8, 8)).state,
            CellState::Revealed
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
        // Reveal a numeric Cell so the game stays Playing (no cascade, no win).
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
