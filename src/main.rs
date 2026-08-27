mod core;
mod server;

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::core::{Difficulty, Features, Game, GameConfig, Seed};

/// Command-line options for the game server.
#[derive(Parser)]
#[command(about = "A Minesweeper web app: a Rust game server with a TypeScript frontend.")]
struct Cli {
    /// Enable the Prank Feature: the First Click of every game is always a
    /// Mine. The UI never indicates the Feature is active (ADR-0002).
    /// Mutually exclusive with `--seed`: Prank is a joke easter egg and
    /// non-seedable.
    #[arg(long, conflicts_with = "seed")]
    prank: bool,

    /// Pin one Seed for every game of this session: each Difficulty
    /// reproduces the same Mine layout. Absent, every New Game draws a
    /// fresh random Seed, printed to the terminal. Mutually exclusive with
    /// `--prank`.
    #[arg(long, conflicts_with = "prank")]
    seed: Option<Seed>,

    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Interface to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() {
    // Logging is initialized before anything else so every startup path is
    // visible; RUST_LOG overrides the default `info` level (issue #27).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let features = if cli.prank {
        Features::prank()
    } else {
        Features::NONE
    };

    // `--prank` maps to the Prank Feature; `--seed` pins the Seed. Both are
    // mutually exclusive (Prank is unseedable); absent both, a fresh Random
    // game per play (issue #100). The Seed is committed (and logged) at the
    // First Click for every game.
    //
    // The session's launch-time intent is fixed here: one game at a time, with
    // the Features and pinned Seed set once at launch (issue #103). The Game's
    // config is the single source of truth — every New Game reuses it, switching
    // only the Difficulty.
    let game = Game::with_config(GameConfig::new(Difficulty::Beginner, features, cli.seed));
    server::log_new_game(&game, "startup");
    let state: Arc<Mutex<Game>> = Arc::new(Mutex::new(game));

    // The built frontend (frontend/dist) is served at the root; unknown
    // paths fall back to index.html so client-side routing never 404s.
    let app = Router::new()
        .route("/state", get(server::get_state))
        .route("/action", post(server::post_action))
        .fallback_service(
            ServeDir::new("frontend/dist").fallback(ServeFile::new("frontend/dist/index.html")),
        )
        .with_state(state);

    let ip: IpAddr = cli.host.parse().expect("invalid host address");
    let addr = SocketAddr::new(ip, cli.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    info!(
        prank = cli.prank,
        seed = cli.seed,
        "Minesweeper web UI at http://{addr}"
    );
    axum::serve(listener, app).await.expect("server error");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prank_and_seed_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["minesweeper", "--prank", "--seed", "42"]).is_err());
        assert!(Cli::try_parse_from(["minesweeper", "--seed", "42", "--prank"]).is_err());
    }

    #[test]
    fn seed_alone_parses() {
        let cli = Cli::try_parse_from(["minesweeper", "--seed", "42"]).unwrap();
        assert_eq!(cli.seed, Some(42));
        assert!(!cli.prank);
    }

    #[test]
    fn prank_alone_parses() {
        let cli = Cli::try_parse_from(["minesweeper", "--prank"]).unwrap();
        assert!(cli.prank);
        assert_eq!(cli.seed, None);
    }
}
