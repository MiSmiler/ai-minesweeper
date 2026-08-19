mod core;
mod server;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::core::{Difficulty, Game, GameMode};
use crate::server::AppState;

/// Command-line options for the game server.
#[derive(Parser)]
#[command(about = "A Minesweeper web app: a Rust game server with a TypeScript frontend.")]
struct Cli {
    /// Prank Mode: the First Click of every game is always a Mine. The UI
    /// never indicates the mode is active (ADR-0002).
    #[arg(long)]
    prank: bool,

    /// Pin one Seed for every game of this session: each Difficulty
    /// reproduces the same Mine layout. Absent, every New Game draws a
    /// fresh random Seed, printed to the terminal.
    #[arg(long)]
    seed: Option<u32>,

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
    let mode = if cli.prank {
        GameMode::Prank
    } else {
        GameMode::Classic
    };

    let seed = cli.seed.unwrap_or_else(rand::random);
    let game = Game::with_seed(Difficulty::Beginner, mode, seed);
    server::log_new_game(&game, "startup");
    let state = Arc::new(AppState {
        game: std::sync::Mutex::new(game),
        mode,
        seed: cli.seed,
    });

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
    let mode_str = match mode {
        GameMode::Classic => "classic",
        GameMode::Prank => "prank",
    };
    info!(
        host = %cli.host,
        port = cli.port,
        mode = mode_str,
        seed,
        "Minesweeper web UI ready"
    );
    axum::serve(listener, app).await.expect("server error");
}
