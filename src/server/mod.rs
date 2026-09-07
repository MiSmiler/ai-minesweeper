//! HTTP server layer: the game state API.
//!
//! A thin adapter over the core interface (ADR-0003). `core.rs` stays a pure
//! logic module with no serde or server dependencies; this module owns the
//! axum handlers and maps core types to the wire DTOs in [`wire`].

mod ai_routes;
pub mod wire;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::ai_adapter::Guide;
use crate::core::{Difficulty, Game, GameState, Position};

use self::wire::{ActionDto, ActionKind, GameSnapshot};

/// The shared server state: the single `Game`, the AI advisor, and the set of
/// live analysis sessions keyed by the frontend-generated `sessionId`. The
/// `/ai/...` routes only read the board (never write `Game`).
pub(crate) struct AppState {
    pub(crate) game: Arc<Mutex<Game>>,
    pub(crate) guide: Guide,
    /// Active `Guide::suggest` cancel tokens, keyed by the `{id}` path segment
    /// (= the frontend `sessionId`). The interrupt route cancels the matching
    /// token to abort the upstream generation and drive the SSE event.
    pub(crate) ai_sessions: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

/// The outcome of applying a player action to the human game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionOutcome {
    /// A New Game replaced the previous one. The committed Seed is on
    /// `game.committed_seed()` (`None` until the First Click, for every game;
    /// a pinned `--seed` then commits to the pinned value, a random game to a
    /// fresh draw).
    NewGame,
    /// A Reveal / Flag / Chord mutated the existing game.
    Applied,
}

/// Assembles the full API onto a `Router`, given the shared `AppState`. The
/// static `/frontend/dist` fallback is attached by `main.rs`.
pub fn routes(state: Arc<AppState>) -> Router {
    let game_router = Router::new()
        .route("/state", get(get_state))
        .route("/action", post(post_action))
        .with_state(state.clone());
    game_router.merge(ai_routes::ai_routes(state))
}

/// Applies an action to the game, replacing it for a new-game. Returns the
/// outcome (`NewGame` for a new-game, `Applied` for a mutation), or an error
/// message for malformed actions (missing coordinates, bad difficulty). A new
/// game reuses the Game's own config via `Game::new_game`, switching only the
/// Difficulty; the committed Seed of a new game is on `game.committed_seed()`.
pub(crate) fn apply_action(game: &mut Game, action: &ActionDto) -> Result<ActionOutcome, String> {
    match action.kind {
        ActionKind::NewGame => {
            let difficulty = match &action.difficulty {
                Some(raw) => Some(Difficulty::parse(raw)?),
                None => None, // None = new_game keeps the current Difficulty
            };
            game.new_game(difficulty);
            Ok(ActionOutcome::NewGame)
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
            Ok(ActionOutcome::Applied)
        }
    }
}

/// Logs a freshly created Game's difficulty at `info` with its `source`. The
/// Seed lifecycle (a committed Seed at `info`, rejected candidates at `debug`)
/// is logged by the engine in `core.rs`; this records only that a game was
/// created. `source` distinguishes the initial game from player-triggered
/// New Games.
pub fn log_new_game(game: &Game, source: &str) {
    info!(difficulty = game.difficulty().as_str(), source, "new game");
}

// --- Handlers ---

pub(crate) async fn get_state(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GameSnapshot>, (StatusCode, String)> {
    let game = state.game.lock().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "game state poisoned".to_string(),
        )
    })?;
    Ok(Json(GameSnapshot::from_game(&game)))
}

pub(crate) async fn post_action(
    State(state): State<Arc<AppState>>,
    Json(action): Json<ActionDto>,
) -> Result<Json<GameSnapshot>, (StatusCode, String)> {
    let mut game = state.game.lock().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "game state poisoned".to_string(),
        )
    })?;
    let before = game.game_state();
    let outcome = apply_action(&mut game, &action).map_err(|e| {
        // Malformed actions are client bugs; the raw payload makes them
        // diagnosable from the log alone.
        warn!(action = ?action, error = %e, "rejected action");
        (StatusCode::BAD_REQUEST, e)
    })?;
    let after = game.game_state();
    // core.rs logs the Seed lifecycle (a committed Seed at info, rejected
    // candidates at debug); here we only record that a new game was created.
    if matches!(outcome, ActionOutcome::NewGame) {
        log_new_game(&game, "player");
    }
    debug!(
        action = ?action,
        from = before.as_str(),
        to = after.as_str(),
        "action applied"
    );
    if before != after && matches!(after, GameState::Won | GameState::Lost) {
        info!(
            game_state = after.as_str(),
            elapsed_secs = game.elapsed().as_secs(),
            flags_remaining = game.flags_remaining(),
            "game over"
        );
    }
    Ok(Json(GameSnapshot::from_game(&game)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Difficulty, Features, Game, GameConfig, GameState, Position};
    use crate::server::wire::{ActionDto, ActionKind, GameSnapshot, PositionDto};

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
    fn reveal_action_places_mines_and_starts_playing() {
        // Preset Mines so the First Click is deterministically safe: #19
        // removed the protection, so a random game could lose on (0, 0).
        // Two Mines adjacent to (0, 0) make it reveal a Number, not cascade.
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            Features::NONE,
            &[Position::new(0, 1), Position::new(1, 0)],
        );
        apply_action(
            &mut game,
            &action(ActionKind::Reveal, Some(0), Some(0), None),
        )
        .unwrap();
        assert_eq!(game.game_state(), GameState::Playing);
    }

    #[test]
    fn new_game_action_switches_difficulty() {
        let mut game =
            Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None));
        apply_action(
            &mut game,
            &action(ActionKind::NewGame, None, None, Some("expert")),
        )
        .unwrap();
        assert_eq!(game.difficulty(), Difficulty::Expert);
        assert_eq!(game.size(), Difficulty::Expert.size());
    }

    #[test]
    fn new_game_without_difficulty_keeps_current() {
        let mut game = Game::with_config(GameConfig::new(Difficulty::Expert, Features::NONE, None));
        apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        assert_eq!(game.difficulty(), Difficulty::Expert);
    }

    #[test]
    fn missing_coordinates_are_rejected() {
        let mut game =
            Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None));
        let err =
            apply_action(&mut game, &action(ActionKind::Reveal, None, None, None)).unwrap_err();
        assert!(err.contains("row and col"));
    }

    #[test]
    fn bad_difficulty_is_rejected() {
        let mut game =
            Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None));
        let err = apply_action(
            &mut game,
            &action(ActionKind::NewGame, None, None, Some("insane")),
        )
        .unwrap_err();
        assert!(err.contains("invalid difficulty"));
    }

    #[test]
    fn prank_first_reveal_loses_with_trigger_in_snapshot() {
        let mut game = Game::with_config(GameConfig::new(
            Difficulty::Beginner,
            Features::prank(),
            None,
        ));
        apply_action(
            &mut game,
            &action(ActionKind::Reveal, Some(0), Some(0), None),
        )
        .unwrap();
        let dto = GameSnapshot::from_game(&game);
        assert_eq!(dto.game_state, "lost");
        assert_eq!(dto.trigger, Some(PositionDto { row: 0, col: 0 }));
        // The wire only ever shows a Lost game — the Prank Feature is invisible on it (ADR-0002).
    }

    #[test]
    fn fixed_seed_is_reused_for_every_new_game() {
        let mut game = Game::with_config(GameConfig::new(
            Difficulty::Beginner,
            Features::NONE,
            Some(42),
        ));
        apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        // A pinned Seed is deferred: committed only at the First Click.
        assert_eq!(game.committed_seed(), None);
        game.reveal(Position::new(0, 0));
        assert_eq!(game.committed_seed(), Some(42));
        apply_action(
            &mut game,
            &action(ActionKind::NewGame, None, None, Some("expert")),
        )
        .unwrap();
        game.reveal(Position::new(0, 0));
        assert_eq!(game.committed_seed(), Some(42));
    }

    #[test]
    fn new_game_without_fixed_seed_draws_fresh_seeds() {
        let mut game =
            Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None));
        apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        game.reveal(Position::new(0, 0));
        let first = game.committed_seed().unwrap();
        apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        game.reveal(Position::new(0, 0));
        assert_ne!(game.committed_seed().unwrap(), first);
    }

    #[test]
    fn new_game_action_reports_a_new_game() {
        let mut game = Game::with_config(GameConfig::new(
            Difficulty::Beginner,
            Features::NONE,
            Some(7),
        ));
        let outcome =
            apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        assert!(matches!(outcome, ActionOutcome::NewGame));
        // A pinned Seed is deferred to the First Click.
        assert_eq!(game.committed_seed(), None);
        let outcome = apply_action(
            &mut game,
            &action(ActionKind::Reveal, Some(0), Some(0), None),
        )
        .unwrap();
        assert!(matches!(outcome, ActionOutcome::Applied));
        assert_eq!(game.committed_seed(), Some(7));
    }

    #[test]
    fn new_game_reuses_session_features_and_seed() {
        let mut game = Game::with_config(GameConfig::new(
            Difficulty::Beginner,
            Features::prank(),
            None,
        ));
        apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        assert_eq!(game.features(), Features::prank());
        // Prank is non-reproducible: the Seed is dropped at the model
        // boundary (ADR-0010), so the committed Seed is a local draw at the
        // First Click, not a pinned value.
        assert_eq!(game.committed_seed(), None);
        apply_action(
            &mut game,
            &action(ActionKind::Reveal, Some(0), Some(0), None),
        )
        .unwrap();
        assert_eq!(game.game_state(), GameState::Lost);
        assert_ne!(game.committed_seed(), Some(5));
    }

    #[test]
    fn new_game_without_seed_is_random_and_unpranked() {
        let mut game =
            Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None));
        apply_action(&mut game, &action(ActionKind::NewGame, None, None, None)).unwrap();
        assert_eq!(game.features(), Features::NONE);
        assert_eq!(game.committed_seed(), None);
    }
}
