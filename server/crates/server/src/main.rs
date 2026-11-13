//! VOIDWRAP RUN authoritative game server.
//!
//! Architecture: one game task owns all simulation state and runs the fixed 20 Hz
//! tick loop. Each WebSocket connection is a pair of small tasks that only exchange
//! bounded channel messages with the game task — no shared mutable state, no locks.

mod content;
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

    // Where to listen. Hosting platforms set PORT and need every interface; locally the default is this
    // computer only. VOIDWRAP_HOST=0.0.0.0 opens the game to other computers (LAN multiplayer).
    let hosted_port: Option<u16> = std::env::var("PORT").ok().and_then(|p| p.parse().ok());
    let port: u16 = hosted_port
        .or_else(|| std::env::var("VOIDWRAP_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(7878);
    let default_host = if hosted_port.is_some() { "0.0.0.0" } else { "127.0.0.1" };
    let host: std::net::IpAddr = std::env::var("VOIDWRAP_HOST")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or_else(|| default_host.parse().unwrap_or(std::net::IpAddr::from([127, 0, 0, 1])));
    let addr = SocketAddr::from((host, port));
    let web_dir = net::find_web_dir();

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
    // Seed draft offers and level layouts from the clock: every run is different.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
        .unwrap_or(0x5eed);
    let state = match game::Game::starting_at(start_level, seed) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("cannot start: {e}");
            std::process::exit(1);
        }
    };
    let game = tokio::spawn(game::run(game_rx, state));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("VOIDWRAP RUN server listening on ws://{addr}/ws");
    match &web_dir {
        Some(dir) => {
            let shown = if host.is_unspecified() || host.is_loopback() {
                "localhost".to_string()
            } else {
                host.to_string()
            };
            tracing::info!("serving the game from {}", dir.display());
            tracing::info!(">>> open http://{shown}:{port} in your browser to play <<<");
            if std::env::var_os("VOIDWRAP_OPEN_BROWSER").is_some() {
                open_browser(&format!("http://localhost:{port}"));
            }
        }
        None => tracing::info!("no built web client found (dev mode): run the web dev server separately"),
    }

    let app = net::router(game_tx, web_dir);
    let serve = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
    });
    if let Err(e) = serve.await {
        tracing::error!("server error: {e}");
    }
    game.abort();
}

/// Best-effort: open the game in the default browser (used by the PLAY launchers). Failure is harmless.
fn open_browser(url: &str) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = result {
        tracing::warn!("could not open a browser ({e}); open {url} yourself");
    }
}
