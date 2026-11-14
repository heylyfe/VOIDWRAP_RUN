#![cfg_attr(all(windows, feature = "desktop"), windows_subsystem = "windows")]

//! VOIDWRAP RUN authoritative game server.
//!
//! Architecture: one game task owns all simulation state and runs the fixed 20 Hz
//! tick loop. Each WebSocket connection is a pair of small tasks that only exchange
//! bounded channel messages with the game task — no shared mutable state, no locks.
//!
//! Desktop builds (`--features desktop`) open a native window instead of a browser,
//! unless `--browser`, `--headless` or `PORT` are used; see `desktop.rs`.

mod content;
#[cfg(feature = "desktop")]
mod desktop;
mod game;
mod host;
mod matchmaker_client;
mod net;
mod run;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

/// Inbound queue from all connections to the game task.
const GAME_INBOX_CAPACITY: usize = 1024;

/// Everything `main` needs before choosing between the windowed and the normal path.
struct Startup {
    /// Address the normal listener binds.
    addr: SocketAddr,
    /// Built web client directory, if found.
    web_dir: Option<PathBuf>,
    /// First level of a new run (dev/QA aid).
    start_level: u32,
    /// True when the server itself listens on all interfaces (`VOIDWRAP_HOST=0.0.0.0` or `PORT` set).
    forced_open: bool,
    /// Port the hosting listener uses (`VOIDWRAP_HOST_PORT`, default 7879).
    host_port: u16,
}

/// Read the environment once and compute the listen address, web directory and ports.
fn startup() -> Startup {
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
    let forced_open = host.is_unspecified();
    // Dev/QA aid: start every run at a later level (e.g. to review each colour theme).
    let start_level: u32 = std::env::var("VOIDWRAP_START_LEVEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .clamp(1, 10_000);
    let host_port = host::parse_host_port(std::env::var("VOIDWRAP_HOST_PORT").ok().as_deref());
    Startup {
        addr,
        web_dir: net::find_web_dir(),
        start_level,
        forced_open,
        host_port,
    }
}

fn main() {
    init_logging();
    let startup = startup();

    // Desktop window mode: server on a background thread, tao/wry window on the main thread.
    #[cfg(feature = "desktop")]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let port_env_set = std::env::var_os("PORT").is_some();
        if desktop::should_open_window(&args, port_env_set) {
            run_desktop(startup);
            return;
        }
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!("could not create the tokio runtime: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = runtime.block_on(run_server(startup, None)) {
        tracing::error!("fatal: {e}");
        std::process::exit(1);
    }
}

/// Desktop path: server on a background thread; window on the main thread; closing the window
/// shuts the server down (best effort) and then exits.
#[cfg(feature = "desktop")]
fn run_desktop(startup: Startup) {
    let port = startup.addr.port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    // The window waits on this after a close so cleanup (UPnP removal) finishes: tao exits the
    // process as soon as its event loop ends, so code after open_window may never run.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let server_thread = std::thread::Builder::new()
        .name("voidwrap-server".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(e) => {
                    tracing::error!("could not create the tokio runtime: {e}");
                    return;
                }
            };
            if let Err(e) = runtime.block_on(run_server(startup, Some(shutdown_rx))) {
                tracing::error!("server stopped with an error: {e}");
            }
            let _ = done_tx.send(());
        });
    let server_thread = match server_thread {
        Ok(thread) => Some(thread),
        Err(e) => {
            tracing::error!("could not start the server thread: {e}");
            None
        }
    };
    desktop::wait_for_server(port, std::time::Duration::from_secs(10));
    desktop::open_window(port, shutdown_tx, done_rx);
    // Best effort: let the server finish graceful shutdown (removes any UPnP mapping).
    if let Some(thread) = server_thread {
        let _ = thread.join();
    }
}

/// Run the server until the shutdown signal (Ctrl-C, or the desktop window closing).
/// Returns an error message when startup fails.
async fn run_server(startup: Startup, window_closed: Option<tokio::sync::oneshot::Receiver<()>>) -> Result<(), String> {
    let Startup {
        addr,
        web_dir,
        start_level,
        forced_open,
        host_port,
    } = startup;

    if start_level != 1 {
        tracing::warn!("VOIDWRAP_START_LEVEL={start_level}: runs start at level {start_level} (dev/QA only)");
    }
    let (game_tx, game_rx) = mpsc::channel(GAME_INBOX_CAPACITY);
    // Seed draft offers and level layouts from the clock: every run is different.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
        .unwrap_or(0x5eed);
    let state = game::Game::starting_at(start_level, seed).map_err(|e| format!("cannot start: {e}"))?;
    let game_state = Arc::new(matchmaker_client::GameStateHandle::new());
    let game = tokio::spawn(game::run(game_rx, state, Arc::clone(&game_state)));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("cannot bind {addr}: {e}"))?;
    tracing::info!("VOIDWRAP RUN server listening on ws://{addr}/ws");
    match &web_dir {
        Some(dir) => {
            let shown = if addr.ip().is_unspecified() || addr.ip().is_loopback() {
                "localhost".to_string()
            } else {
                addr.ip().to_string()
            };
            tracing::info!("serving the game from {}", dir.display());
            tracing::info!(">>> open http://{shown}:{} in your browser to play <<<", addr.port());
            if std::env::var_os("VOIDWRAP_OPEN_BROWSER").is_some() {
                open_browser(&format!("http://localhost:{}", addr.port()));
            }
        }
        None => tracing::info!("no built web client found (dev mode): run the web dev server separately"),
    }

    let mut host_manager = host::HostManager::new(addr.port(), host_port, forced_open);
    host_manager.set_game_state(Arc::clone(&game_state));
    let host_manager = Arc::new(host_manager);
    {
        let status = host_manager.status().await;
        if let Ok(json) = serde_json::to_string(&status) {
            tracing::info!("host status at startup: {json}");
        }
        if status.hosting {
            tracing::info!(
                "hosting is ON: friends can reach this computer on port {} (started with VOIDWRAP_HOST=0.0.0.0)",
                status.port
            );
        } else {
            tracing::info!("hosting is off: only this computer can play. Turn it on in the game.");
        }
    }
    // Forced-open hosts (the Render deployment) are listed on the matchmaker from the start;
    // normal hosts register when the hosting toggle settles. Never fails the server.
    host_manager.register_if_forced_open().await;

    let app = net::router(game_tx, web_dir, Arc::clone(&host_manager));
    let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal(window_closed));
    if let Err(e) = serve.await {
        tracing::error!("server error: {e}");
    }
    // Best effort: close the hosting listener and remove any UPnP mapping before exit.
    let status = host_manager.stop_hosting().await;
    if let Ok(json) = serde_json::to_string(&status) {
        tracing::info!("host status at shutdown: {json}");
    }
    game.abort();
    Ok(())
}

/// Wait for Ctrl-C, or for the desktop window to close.
async fn shutdown_signal(window_closed: Option<tokio::sync::oneshot::Receiver<()>>) {
    match window_closed {
        Some(rx) => {
            tokio::select! {
                res = tokio::signal::ctrl_c() => {
                    if let Err(e) = res {
                        tracing::warn!("ctrl-c handler failed: {e}");
                    } else {
                        tracing::info!("shutting down (Ctrl-C)");
                    }
                }
                _ = rx => tracing::info!("the window was closed; shutting down"),
            }
        }
        None => {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        }
    }
}

/// Normal logging: stdout/console (as before).
#[cfg(not(all(feature = "desktop", target_os = "windows")))]
fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
}

/// Windows desktop builds have no console; log to `voidwrap.log` next to the executable so
/// problems can be reported. The file is truncated on each start.
#[cfg(all(feature = "desktop", target_os = "windows"))]
fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let path = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("voidwrap.log")))
        .unwrap_or_else(|| PathBuf::from("voidwrap.log"));
    match std::fs::File::create(&path) {
        Ok(file) => {
            let shared = std::sync::Arc::new(std::sync::Mutex::new(file));
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(move || SharedFileWriter(std::sync::Arc::clone(&shared)))
                .init();
            tracing::info!("logging to {}", path.display());
        }
        Err(e) => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .init();
            tracing::warn!("could not open {} for logging ({e}); logging to stderr", path.display());
        }
    }
}

/// `std::io::Write` adapter around a shared file handle, for tracing's `with_writer`.
#[cfg(all(feature = "desktop", target_os = "windows"))]
struct SharedFileWriter(std::sync::Arc<std::sync::Mutex<std::fs::File>>);

#[cfg(all(feature = "desktop", target_os = "windows"))]
impl std::io::Write for SharedFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.lock() {
            Ok(mut file) => std::io::Write::write(&mut *file, buf),
            Err(_) => Err(std::io::Error::other("voidwrap.log lock poisoned")),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.lock() {
            Ok(mut file) => std::io::Write::flush(&mut *file),
            Err(_) => Err(std::io::Error::other("voidwrap.log lock poisoned")),
        }
    }
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
