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

    /// Parses a difficulty name, also accepting the classic 1/2/3 numbers.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "beginner" | "1" => Ok(Difficulty::Beginner),
            "intermediate" | "2" => Ok(Difficulty::Intermediate),
            "expert" | "3" => Ok(Difficulty::Expert),
            other => Err(format!(
                "invalid difficulty '{other}': expected beginner|intermediate|expert or 1|2|3"
            )),
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

/// An opt-in behavior that modifies how a game plays, independent of the
/// rule set and the Seed (ADR-0010). The only Feature is Prank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// The First Click is always a Mine, losing the game instantly.
    Prank,
}

/// The set of opt-in Features enabled on a Game; presence means the
/// behavior is on. Fixed at Game creation (ADR-0010).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Features(u8);

impl Features {
    /// The empty feature set.
    pub const NONE: Self = Self(0);

    const PRANK: u8 = 1 << 0;

    /// A set with only the Prank Feature.
    pub fn prank() -> Self {
        Self(Self::PRANK)
    }

    /// Whether the given Feature is enabled (present in the set).
    pub fn contains(self, feature: Feature) -> bool {
        match feature {
            Feature::Prank => self.0 & Self::PRANK != 0,
        }
    }
}

/// The immutable configuration a Game is created from: a Difficulty, a set
/// of Features, and an optional pinned Seed (ADR-0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameConfig {
    pub difficulty: Difficulty,
    pub features: Features,
    /// `Some` pins one Seed for every game (reproduce the layout); `None`
    /// draws a fresh random Seed per game. A Prank game always drops it
    /// (model-level mutual exclusion: Prank is unseedable).
    pub pinned_seed: Option<Seed>,
}

impl GameConfig {
    /// Builds a `GameConfig` from a Difficulty, the session's Features, and
    /// the pinned Seed (issue #100). A `Some(pinned_seed)` pins that exact
    /// Seed for the layout; `None` makes a fresh Random game (a new Seed
    /// drawn per play). Prank is mutually exclusive with a pinned Seed: a
    /// Prank game drops any passed Seed, since it is a joke easter egg and
    /// non-reproducible.
    pub fn new(difficulty: Difficulty, features: Features, pinned_seed: Option<Seed>) -> Self {
        let pinned_seed = if features.contains(Feature::Prank) {
            None
        } else {
            pinned_seed
        };
        Self {
            difficulty,
            features,
            pinned_seed,
        }
    }
}

/// The state of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    /// Board exists; no Cell Revealed, Timer not started. Mines are placed
    /// at the First Click for every game — Pinned, Random, and Prank alike.
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

/// A Minesweeper game: standard rules plus optional Features and an optional
/// pinned Seed (ADR-0010). Every game places its Mines at the First Click: a
/// Pinned Seed reproduces the exact layout (ADR-0004); a Random game
/// regenerates the Seed until the First Click is safe (ADR-0009); a Prank
/// game forces the clicked Cell in (ADR-0002).
#[derive(Debug, Clone)]
pub struct Game {
    config: GameConfig,
    size: BoardSize,
    state: GameState,
    /// Mine positions; `None` until placed at the First Click.
    mines: Option<Vec<Position>>,
    /// The committed layout Seed; `Some` exactly when `mines` is `Some`.
    committed_seed: Option<Seed>,
    /// CellState per Cell, indexed by Position (row-major).
    cell_states: Vec<CellState>,
    flags: usize,
    /// The number of Mines on the Board, fixed at creation from the
    /// Difficulty's Mine count (the "recipe"); it never changes during the
    /// game. Also the zero reference of the Flag counter.
    mine_count: usize,
    /// Trigger Mine position; `None` unless the game is Lost.
    trigger: Option<Position>,
    started_at: Option<Instant>,
    elapsed_at_end: Option<Duration>,
}

impl Game {
    /// Creates a game from an explicit `GameConfig` (canonical constructor,
    /// ADR-0010). Mines are deferred to the First Click for every game —
    /// Pinned, Random, and Prank alike.
    pub fn with_config(config: GameConfig) -> Self {
        Self::build(config)
    }

    /// Builds a game from its configuration. Pure construction: no Mines are
    /// placed and no Seed is committed; they land at the First Click.
    fn build(config: GameConfig) -> Self {
        let size = config.difficulty.size();
        let mine_count = config.difficulty.mine_count();
        Self {
            config,
            size,
            state: GameState::Ready,
            mines: None,
            committed_seed: None,
            cell_states: vec![CellState::Hidden; size.rows * size.cols],
            flags: 0,
            mine_count,
            trigger: None,
            started_at: None,
            elapsed_at_end: None,
        }
    }

    /// Starts a fresh game from this game's config, optionally switching
    /// Difficulty. Reuses the same Features and pinned Seed set at creation,
    /// so the session's launch-time intent is preserved. `None` keeps the
    /// current Difficulty.
    pub fn new_game(&mut self, difficulty: Option<Difficulty>) {
        let difficulty = difficulty.unwrap_or(self.config.difficulty);
        let config = GameConfig::new(difficulty, self.config.features, self.config.pinned_seed);
        *self = Self::build(config);
    }

    /// Creates a game with a preset Mine list (test use). For a non-Prank
    /// game the caller is responsible for first-click safety: Mines on or
    /// adjacent to the first-clicked Cell are NOT filtered. A preset is a
    /// deterministic non-Prank layout — a Prank game ignores it and samples
    /// its own Mines at the First Click. The mine count is still set from
    /// the Difficulty (the recipe) and is independent of the preset length.
    #[cfg(test)]
    pub fn with_mines(difficulty: Difficulty, features: Features, mines: &[Position]) -> Self {
        let mut game = Self::with_config(GameConfig {
            difficulty,
            features,
            pinned_seed: None,
        });
        game.mines = Some(mines.to_vec());
        game.committed_seed = Some(0);
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
        self.config.difficulty
    }

    /// The set of Features enabled on this game (ADR-0010).
    #[allow(dead_code)]
    pub fn features(&self) -> Features {
        self.config.features
    }

    /// The committed layout Seed, or `None` while the Mines aren't placed. It
    /// is `Some` exactly when `mines` is `Some` — the invariant maintained by
    /// `commit_mines` — and is set at the First Click for every game: a
    /// Pinned Seed commits to the pinned value (ADR-0004), a Random game to
    /// the accepted candidate (ADR-0009), a Prank game to a local draw
    /// (ADR-0002, non-reproducible). `None` at `Ready` for every game.
    // `committed_seed` is the public read seam. Its only caller today is a
    // test; the server reads a New Game's committed Seed off the freshly
    // built `Game`, so the non-test build would otherwise flag it as dead. It
    // is the authoritative read side and must stay.
    #[allow(dead_code)]
    pub fn committed_seed(&self) -> Option<Seed> {
        self.committed_seed
    }

    /// Fixes the Mine layout, committing the Seed: the moment a Mine list
    /// becomes `Some` is exactly when the Seed is committed, so this is the
    /// single write-side seam for that event (read it back with
    /// `committed_seed`, and it logs the commit at `info`). It maintains the
    /// invariant `committed_seed == Some ⟺ mines == Some`.
    fn commit_mines(&mut self, mines: Vec<Position>, committed_seed: Seed, at: &str) {
        self.mines = Some(mines);
        self.committed_seed = Some(committed_seed);
        info!(
            seed = committed_seed,
            difficulty = self.config.difficulty.as_str(),
            "seed committed {at}"
        );
    }

    /// The placed Mines, if any; `None` until the First Click (for every
    /// game). Test use.
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
    /// or it is out of bounds. The first Reveal of a game places the Mines
    /// (every game — Pinned, Random, Prank, or a test preset) and starts the
    /// Timer.
    pub fn reveal(&mut self, pos: Position) {
        if !self.can_operate(pos) {
            return;
        }
        if self.cell_state(pos) != CellState::Hidden {
            return;
        }
        if self.state == GameState::Ready {
            // The First Click places Mines for every game (ADR-0010): Prank
            // forces the clicked Cell in; a pinned `--seed` reproduces the
            // exact layout; a Random game regenerates until the First Click
            // is safe. The non-Prank branch only runs when no test preset
            // already placed Mines (`mines.is_none()`).
            if self.config.features.contains(Feature::Prank) {
                self.place_prank_mines(pos);
            } else if self.mines.is_none() {
                match self.config.pinned_seed {
                    Some(seed) => self.place_mines_by_pinned_seed(seed),
                    None => self.place_mines_for_safe_first_click(pos),
                }
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
    /// enumeration-and-shuffle (issue #18). Pure: it has no `forced` notion —
    /// the Prank path's forced Cell is handled by the caller.
    fn sample_mines(size: BoardSize, count: usize, seed: Seed) -> Vec<Position> {
        let total = size.rows * size.cols;
        let mut rng = StdRng::seed_from_u64(seed as u64);
        index::sample(&mut rng, total, count)
            .into_iter()
            .map(|i| Position::new(i / size.cols, i % size.cols))
            .collect()
    }

    /// Places the Mines of a Prank game at the First Click (ADR-0002): the
    /// First Clicked Cell is forced into the Mine list. The layout is sampled
    /// from a fresh local Seed with the clicked Cell swapped in if absent, so
    /// a Prank game is non-reproducible.
    fn place_prank_mines(&mut self, first_click: Position) {
        let candidate = rand::random();
        let mut mines = Self::sample_mines(self.size, self.mine_count, candidate);
        if !mines.contains(&first_click) {
            mines.pop();
            mines.push(first_click);
        }
        self.commit_mines(mines, candidate, "at first click");
    }

    /// Places Mines for a pinned `--seed` at the First Click (ADR-0004): the
    /// exact layout is sampled from the Seed, leaving the First Click
    /// unprotected.
    fn place_mines_by_pinned_seed(&mut self, seed: Seed) {
        let mines = Self::sample_mines(self.size, self.mine_count, seed);
        self.commit_mines(mines, seed, "at first click");
    }

    /// Makes a random game's First Click safe (ADR-0009): the Seed is re-drawn
    /// until the clicked Cell's 3x3 is Mine-free, and the accepted Seed (with
    /// its layout) becomes the game's Mines.
    fn place_mines_for_safe_first_click(&mut self, first_click: Position) {
        let region: Vec<Position> = Self::neighbors(self.size, first_click)
            .chain(std::iter::once(first_click))
            .collect();
        let mut candidate = rand::random();
        loop {
            let mines = Self::sample_mines(self.size, self.mine_count, candidate);
            if !region.iter().any(|p| mines.contains(p)) {
                self.commit_mines(mines, candidate, "at first click");
                return;
            }
            debug!(
                seed = candidate,
                "seed candidate rejected; first click not safe"
            );
            candidate = rand::random();
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
mod tests;
