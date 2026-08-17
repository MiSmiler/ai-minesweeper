//! HTTP server layer: the game state API and the wire DTOs.
//!
//! A thin adapter over the core interface (ADR-0003). `core.rs` stays a pure
//! logic module with no serde or server dependencies; this module owns the
//! JSON wire format and maps core types to DTOs.

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::core::{CellContent, CellState, Difficulty, Game, GameMode, GameState, Position};

/// The shared server state: the single live Game plus the fixed GameMode.
/// Mirrors the terminal `App`: one game at a time, mode set once at launch.
pub struct AppState {
    pub game: Mutex<Game>,
    pub mode: GameMode,
}

// --- Wire DTOs ---

/// The full game state the client renders from.
#[derive(Serialize)]
pub struct StateDto {
    pub game_state: &'static str,
    pub difficulty: &'static str,
    pub rows: usize,
    pub cols: usize,
    pub flags_remaining: i32,
    pub elapsed_secs: u64,
    /// The Trigger Mine of a Lost game; `None` otherwise.
    pub trigger: Option<PosDto>,
    /// One entry per Cell, row-major.
    pub cells: Vec<CellDto>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct PosDto {
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
#[derive(Deserialize)]
pub struct ActionDto {
    #[serde(rename = "type")]
    pub kind: ActionKind,
    pub row: Option<usize>,
    pub col: Option<usize>,
    /// `new-game` only; absent keeps the current difficulty.
    pub difficulty: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    Reveal,
    Flag,
    Chord,
    NewGame,
}

// --- Core mapping ---

fn game_state_str(state: GameState) -> &'static str {
    match state {
        GameState::Ready => "ready",
        GameState::Playing => "playing",
        GameState::Won => "won",
        GameState::Lost => "lost",
    }
}

fn difficulty_str(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Beginner => "beginner",
        Difficulty::Intermediate => "intermediate",
        Difficulty::Expert => "expert",
    }
}

fn cell_state_str(state: CellState) -> &'static str {
    match state {
        CellState::Hidden => "hidden",
        CellState::Flagged => "flagged",
        CellState::Revealed => "revealed",
    }
}

/// Builds the wire snapshot of a game. Pure function of the `Game`, so
/// handlers and tests share it.
pub fn snapshot(game: &Game) -> StateDto {
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
                state: cell_state_str(view.state),
                content,
            });
        }
    }
    let trigger = if game.game_state() == GameState::Lost {
        let size = game.size();
        (0..size.rows)
            .flat_map(|row| (0..size.cols).map(move |col| Position::new(row, col)))
            .find(|&pos| game.is_trigger(pos))
            .map(|pos| PosDto {
                row: pos.row,
                col: pos.col,
            })
    } else {
        None
    };
    StateDto {
        game_state: game_state_str(game.game_state()),
        difficulty: difficulty_str(game.difficulty()),
        rows: size.rows,
        cols: size.cols,
        flags_remaining: game.flags_remaining(),
        elapsed_secs: game.elapsed().as_secs(),
        trigger,
        cells,
    }
}

/// Applies an action to the game, replacing it for a new-game. Returns an
/// error message for malformed actions (missing coordinates, bad difficulty).
pub fn apply_action(game: &mut Game, mode: GameMode, action: &ActionDto) -> Result<(), String> {
    match action.kind {
        ActionKind::NewGame => {
            let difficulty = match &action.difficulty {
                Some(raw) => parse_difficulty(raw)?,
                None => game.difficulty(),
            };
            *game = Game::new(difficulty, mode);
            Ok(())
        }
        ActionKind::Reveal | ActionKind::Flag | ActionKind::Chord => {
            let (row, col) = match (action.row, action.col) {
                (Some(row), Some(col)) => (row, col),
                _ => return Err("action requires row and col".into()),
            };
            let pos = Position::new(row, col);
            match action.kind {
                ActionKind::Reveal => game.reveal(pos),
                ActionKind::Flag => game.toggle_flag(pos),
                ActionKind::Chord => game.chord(pos),
                ActionKind::NewGame => unreachable!(),
            }
            Ok(())
        }
    }
}

/// Parses a difficulty name, also accepting the classic 1/2/3 numbers.
pub fn parse_difficulty(s: &str) -> Result<Difficulty, String> {
    match s.to_ascii_lowercase().as_str() {
        "beginner" | "1" => Ok(Difficulty::Beginner),
        "intermediate" | "2" => Ok(Difficulty::Intermediate),
        "expert" | "3" => Ok(Difficulty::Expert),
        other => Err(format!(
            "invalid difficulty '{other}': expected beginner|intermediate|expert or 1|2|3"
        )),
    }
}

// --- Handlers ---

pub async fn get_state(State(app): State<Arc<AppState>>) -> Json<StateDto> {
    Json(snapshot(&app.game.lock().unwrap()))
}

pub async fn post_action(
    State(app): State<Arc<AppState>>,
    Json(action): Json<ActionDto>,
) -> Result<Json<StateDto>, (StatusCode, String)> {
    let mut game = app.game.lock().unwrap();
    apply_action(&mut game, app.mode, &action).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(snapshot(&game)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(
        kind: ActionKind,
        row: Option<usize>,
        col: Option<usize>,
        difficulty: Option<&str>,
    ) -> ActionDto {
        ActionDto {
            kind,
            row,
            col,
            difficulty: difficulty.map(str::to_owned),
        }
    }

    #[test]
    fn snapshot_reflects_a_fresh_game() {
        let game = Game::new(Difficulty::Beginner, GameMode::Classic);
        let dto = snapshot(&game);
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
    fn reveal_action_places_mines_and_starts_playing() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        apply_action(
            &mut game,
            GameMode::Classic,
            &action(ActionKind::Reveal, Some(0), Some(0), None),
        )
        .unwrap();
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn new_game_action_switches_difficulty() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        apply_action(
            &mut game,
            GameMode::Classic,
            &action(ActionKind::NewGame, None, None, Some("expert")),
        )
        .unwrap();
        assert_eq!(game.difficulty(), Difficulty::Expert);
        assert_eq!(game.size(), Difficulty::Expert.size());
    }

    #[test]
    fn new_game_without_difficulty_keeps_current() {
        let mut game = Game::new(Difficulty::Expert, GameMode::Classic);
        apply_action(
            &mut game,
            GameMode::Classic,
            &action(ActionKind::NewGame, None, None, None),
        )
        .unwrap();
        assert_eq!(game.difficulty(), Difficulty::Expert);
    }

    #[test]
    fn missing_coordinates_are_rejected() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        let err = apply_action(
            &mut game,
            GameMode::Classic,
            &action(ActionKind::Reveal, None, None, None),
        )
        .unwrap_err();
        assert!(err.contains("row and col"));
    }

    #[test]
    fn bad_difficulty_is_rejected() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Classic);
        let err = apply_action(
            &mut game,
            GameMode::Classic,
            &action(ActionKind::NewGame, None, None, Some("insane")),
        )
        .unwrap_err();
        assert!(err.contains("invalid difficulty"));
    }

    #[test]
    fn prank_mode_first_reveal_loses_with_trigger_in_snapshot() {
        let mut game = Game::new(Difficulty::Beginner, GameMode::Prank);
        apply_action(
            &mut game,
            GameMode::Prank,
            &action(ActionKind::Reveal, Some(0), Some(0), None),
        )
        .unwrap();
        let dto = snapshot(&game);
        assert_eq!(dto.game_state, "lost");
        assert_eq!(dto.trigger, Some(PosDto { row: 0, col: 0 }));
        // The wire only ever shows a Lost game — the mode itself is invisible (ADR-0002).
    }

    #[test]
    fn snapshot_maps_mine_and_number_content() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(0, 0));
        let dto = snapshot(&game);
        assert_eq!(dto.game_state, "lost");
        assert_eq!(dto.trigger, Some(PosDto { row: 0, col: 0 }));
        assert_eq!(dto.cells[0].state, "revealed");
        assert_eq!(dto.cells[0].content, Some(ContentDto::Mine));

        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 0)],
        );
        game.reveal(Position::new(1, 1)); // one neighbor Mine
        let dto = snapshot(&game);
        let idx = dto.cols + 1;
        assert_eq!(dto.cells[idx].state, "revealed");
        assert_eq!(dto.cells[idx].content, Some(ContentDto::Number(1)));
    }

    #[test]
    fn content_serializes_to_the_wire_shape() {
        assert_eq!(
            serde_json::to_string(&ContentDto::Mine).unwrap(),
            "\"mine\""
        );
        assert_eq!(serde_json::to_string(&ContentDto::Number(3)).unwrap(), "3");
    }
}
