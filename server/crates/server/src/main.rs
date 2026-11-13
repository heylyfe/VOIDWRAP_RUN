//! VOIDWRAP RUN authoritative game server.
//!
//! Architecture: one game task owns all simulation state and runs the fixed 20 Hz
//! tick loop. Each WebSocket connection is a pair of small tasks that only exchange
//! bounded channel messages with the game task — no shared mutable state, no locks.

mod game;
mod net;
mod run;

use std::net::SocketAddr;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

/// Inbound queue from all connections to the game task.
const GAME_INBOX_CAPACITY: usize = 1024;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let port: u16 = std::env::var("VOIDWRAP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7878);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let (game_tx, game_rx) = mpsc::channel(GAME_INBOX_CAPACITY);
    // Dev/QA aid: start every run at a later level (e.g. to review each colour theme).
    let start_level: u32 = std::env::var("VOIDWRAP_START_LEVEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .clamp(1, 10_000);
    if start_level != 1 {
        tracing::warn!("VOIDWRAP_START_LEVEL={start_level}: runs start at level {start_level} (dev/QA only)");
    }
    let game = tokio::spawn(game::run(game_rx, start_level));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("VOIDWRAP RUN server listening on ws://{addr}/ws");

    let app = net::router(game_tx);
    let serve = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
    });
    if let Err(e) = serve.await {
        tracing::error!("server error: {e}");
    }
    game.abort();
}
