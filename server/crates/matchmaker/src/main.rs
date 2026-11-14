//! VOIDWRAP RUN matchmaker binary: serves the frozen lobby contract over HTTP.
//!
//! Binds `0.0.0.0:$PORT` (default 8080) and sweeps expired registrations every 10 s.

use matchmaker::{Registry, serve, spawn_sweeper};

#[tokio::main]
async fn main() {
    let registry = Registry::new();
    // Detached on purpose: the sweeper lives as long as the process does.
    let _sweeper = spawn_sweeper(registry.clone());
    if let Err(error) = serve(registry).await {
        eprintln!("matchmaker stopped: {error}");
        std::process::exit(1);
    }
}
