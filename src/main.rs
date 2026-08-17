mod core;
mod server;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use tower_http::services::{ServeDir, ServeFile};

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

    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Interface to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let mode = if cli.prank {
        GameMode::Prank
    } else {
        GameMode::Classic
    };

    let state = Arc::new(AppState {
        game: std::sync::Mutex::new(Game::new(Difficulty::Beginner, mode)),
        mode,
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
    println!("Minesweeper web UI at http://{addr}");
    axum::serve(listener, app).await.expect("server error");
}
