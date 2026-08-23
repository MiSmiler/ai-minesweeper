//! The AI HTTP routes.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::ai::client::{AiConfig, DeepSeekClient};
use crate::ai::session::{DEFAULT_MAX_TURNS, SessionResult, run_session};
use crate::ai::tools::board_snapshot_text;
use crate::server::AppState;

/// `POST /ai/analyze` — runs a non-streaming DeepSeek session that reads the
/// current Board via the `read_board` tool and returns the transcript of the
/// model's turns. Requires `MY_DS_API_KEY` at startup; otherwise it is
/// disabled with a clear error.
pub async fn analyze(
    State(app): State<Arc<AppState>>,
) -> Result<Json<SessionResult>, (StatusCode, String)> {
    let config = app.ai.as_ref().ok_or((
        StatusCode::BAD_REQUEST,
        "MY_DS_API_KEY is not set; AI analysis is disabled".to_string(),
    ))?;

    let client = DeepSeekClient::new(AiConfig {
        api_key: config.api_key.clone(),
        model: config.model.clone(),
    });

    // Reads the current Board on demand; the game lock is held only for the
    // brief synchronous snapshot, never across the DeepSeek await.
    let game = Arc::clone(&app);
    let board_text = move || match game.game.lock() {
        Ok(game) => board_snapshot_text(&game),
        Err(_) => "board unavailable (game state poisoned)".to_string(),
    };

    let result = run_session(&client, &config.model, "low", board_text, DEFAULT_MAX_TURNS)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("DeepSeek session failed: {e}"),
            )
        })?;

    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::core::{Difficulty, Game, GameMode};

    use super::*;

    #[tokio::test]
    async fn analyze_without_key_returns_bad_request() {
        let app = Arc::new(AppState {
            game: Mutex::new(Game::new(Difficulty::Beginner, GameMode::Classic)),
            mode: GameMode::Classic,
            seed: None,
            ai: None,
        });
        let res = analyze(State(app)).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().0, StatusCode::BAD_REQUEST);
    }
}
