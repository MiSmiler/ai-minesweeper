//! Wire DTOs: the JSON shapes exchanged with the frontend.
//!
//! The wire contract has its own home here. The structs are pure data with
//! serde derives; `GameSnapshot::from_game` maps a `core::Game` into this
//! shape. This module depends on `crate::core` for that mapping and on
//! `serde`, but not on axum (the handlers live in `server::mod.rs`).

use serde::{Deserialize, Serialize};

use crate::core::{CellContent, Game, GameState, Position};

// --- Wire DTOs ---

/// The full game state the client renders from.
#[derive(Serialize)]
pub struct GameSnapshot {
    pub game_state: &'static str,
    pub difficulty: &'static str,
    pub rows: usize,
    pub cols: usize,
    pub flags_remaining: i32,
    pub elapsed_secs: u64,
    /// The Trigger Mine of a Lost game; `None` otherwise.
    pub trigger: Option<PositionDto>,
    /// One entry per Cell, row-major.
    pub cells: Vec<CellDto>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct PositionDto {
    pub row: usize,
    pub col: usize,
}

#[derive(Serialize)]
pub struct CellDto {
    pub state: &'static str,
    /// `Some` only for Revealed Cells: `"mine"` or the neighbor count.
    pub content: Option<ContentDto>,
}

/// Untagged on the wire: the string `"mine"` or a plain number.
#[derive(Debug, PartialEq, Eq)]
pub enum ContentDto {
    Mine,
    Number(u8),
}

impl Serialize for ContentDto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ContentDto::Mine => serializer.serialize_str("mine"),
            ContentDto::Number(n) => serializer.serialize_u8(*n),
        }
    }
}

/// A player action from the client.
#[derive(Debug, Deserialize)]
pub struct ActionDto {
    #[serde(rename = "type")]
    pub kind: ActionKind,
    pub row: Option<usize>,
    pub col: Option<usize>,
    /// `new-game` only; absent keeps the current difficulty.
    pub difficulty: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    Reveal,
    Flag,
    Chord,
    NewGame,
}

// --- Core mapping ---

impl GameSnapshot {
    /// Builds the wire snapshot of a game. Pure function of the `Game`, so
    /// handlers and tests share it.
    pub(crate) fn from_game(game: &Game) -> Self {
        let size = game.size();
        let mut cells = Vec::with_capacity(size.rows * size.cols);
        for row in 0..size.rows {
            for col in 0..size.cols {
                let pos = Position::new(row, col);
                let view = game.cell_view(pos);
                let content = view.content.map(|c| match c {
                    CellContent::Number(n) => ContentDto::Number(n),
                    CellContent::Mine => ContentDto::Mine,
                });
                cells.push(CellDto {
                    state: view.state.as_str(),
                    content,
                });
            }
        }
        // trigger is Some iff the game is Lost: `lose()` sets both together and
        // is the only writer of either (pinned by the assert below).
        debug_assert_eq!(
            game.trigger().is_some(),
            game.game_state() == GameState::Lost
        );
        let trigger = game.trigger().map(|pos| PositionDto {
            row: pos.row,
            col: pos.col,
        });
        GameSnapshot {
            game_state: game.game_state().as_str(),
            difficulty: game.difficulty().as_str(),
            rows: size.rows,
            cols: size.cols,
            flags_remaining: game.flags_remaining(),
            elapsed_secs: game.elapsed().as_secs(),
            trigger,
            cells,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Difficulty, Features, GameConfig};

    #[test]
    fn content_serializes_to_the_wire_shape() {
        assert_eq!(
            serde_json::to_string(&ContentDto::Mine).unwrap(),
            "\"mine\""
        );
        assert_eq!(serde_json::to_string(&ContentDto::Number(3)).unwrap(), "3");
    }

    #[test]
    fn from_game_reflects_a_fresh_game() {
        let game = Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None));
        let dto = GameSnapshot::from_game(&game);
        assert_eq!(dto.game_state, "ready");
        assert_eq!(dto.difficulty, "beginner");
        assert_eq!(dto.rows, 9);
        assert_eq!(dto.cols, 9);
        assert_eq!(dto.flags_remaining, 10);
        assert_eq!(dto.elapsed_secs, 0);
        assert_eq!(dto.trigger, None);
        assert_eq!(dto.cells.len(), 81);
        assert!(
            dto.cells
                .iter()
                .all(|c| c.state == "hidden" && c.content.is_none())
        );
    }

    #[test]
    fn from_game_maps_mine_and_number_content() {
        let mut game =
            Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
        game.reveal(Position::new(0, 0));
        let dto = GameSnapshot::from_game(&game);
        assert_eq!(dto.game_state, "lost");
        assert_eq!(dto.trigger, Some(PositionDto { row: 0, col: 0 }));
        assert_eq!(dto.cells[0].state, "revealed");
        assert_eq!(dto.cells[0].content, Some(ContentDto::Mine));

        let mut game =
            Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
        game.reveal(Position::new(1, 1)); // one neighbor Mine
        let dto = GameSnapshot::from_game(&game);
        let idx = dto.cols + 1;
        assert_eq!(dto.cells[idx].state, "revealed");
        assert_eq!(dto.cells[idx].content, Some(ContentDto::Number(1)));
    }

    #[test]
    fn from_game_flags_all_mines_on_won() {
        let mut game =
            Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
        let size = Difficulty::Beginner.size();
        for row in 0..size.rows {
            for col in 0..size.cols {
                if Position::new(row, col) != Position::new(0, 0) {
                    game.reveal(Position::new(row, col));
                }
            }
        }
        let dto = GameSnapshot::from_game(&game);
        assert_eq!(dto.game_state, "won");
        assert_eq!(dto.flags_remaining, 0);
        // The lone Mine is serialized as a Flag on the Won board.
        assert_eq!(dto.cells[0].state, "flagged");
        assert_eq!(dto.cells[0].content, None);
    }
}
