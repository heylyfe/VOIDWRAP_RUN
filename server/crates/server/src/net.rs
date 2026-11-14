//! WebSocket transport. Validates framing and decodes client messages at the boundary;
//! everything past this point only sees well-formed `ClientMsg` values.
//!
//! Also serves the loopback-only HTTP API for hosting for friends (`GET`/`POST /api/host`);
//! see `host.rs` for the hosting logic and the JSON shape.

use crate::game::{ConnId, GameEvent, OUTBOX_CAPACITY};
use crate::host::{self, HostManager, HostRequest};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use protocol::MAX_CLIENT_MSG_BYTES;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tower_http::services::{ServeDir, ServeFile};

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

/// State shared by every route: the game inbox, the web client directory and the hosting controller.
#[derive(Clone)]
pub struct AppState {
    /// Sender into the game task.
    pub game_tx: mpsc::Sender<GameEvent>,
    /// Built web client, reused when the hosting listener serves the game.
    pub web_dir: Option<PathBuf>,
    /// Hosting controller behind `/api/host`.
    pub host: Arc<HostManager>,
}

/// Build the router: the WebSocket game, the health check, the host API and the static web client.
pub fn router(game_tx: mpsc::Sender<GameEvent>, web_dir: Option<PathBuf>, host: Arc<HostManager>) -> Router {
    let state = AppState {
        game_tx,
        web_dir: web_dir.clone(),
        host,
    };
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(|| async { "ok" }))
        .route("/api/host", get(host_get).post(host_post))
        .with_state(state);
    match web_dir {
        // Serve the built web client from the same address, so one program is the whole game.
        Some(dir) => {
            let not_found = ServeFile::new(dir.join("404.html"));
            app.fallback_service(
                ServeDir::new(dir)
                    .append_index_html_on_directories(true)
                    .fallback(not_found),
            )
        }
        None => app,
    }
}

/// Locate the built web client: $VOIDWRAP_WEB_DIR, then `web/` next to the executable (release download),
/// then `web/out` in the source tree (after `pnpm --dir web build`).
pub fn find_web_dir() -> Option<PathBuf> {
    let has_index = |d: &PathBuf| d.join("index.html").is_file();
    if let Some(dir) = std::env::var_os("VOIDWRAP_WEB_DIR").map(PathBuf::from) {
        return has_index(&dir).then_some(dir);
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("web")));
    let candidates = [
        beside_exe,
        Some(PathBuf::from("web/out")),
        Some(PathBuf::from("../web/out")),
    ];
    candidates.into_iter().flatten().find(has_index)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.max_message_size(MAX_CLIENT_MSG_BYTES as usize)
        .max_frame_size(MAX_CLIENT_MSG_BYTES as usize)
        .on_upgrade(move |socket| handle_socket(socket, state.game_tx))
}

/// `GET /api/host`: current hosting status. Loopback only; friends can never call this.
async fn host_get(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Response {
    if !host::is_loopback_peer(&peer) {
        tracing::warn!("refusing GET /api/host from non-loopback peer {peer}");
        return StatusCode::FORBIDDEN.into_response();
    }
    Json(state.host.status().await).into_response()
}

/// `POST /api/host` with `{ "enable": bool }`: start or stop hosting. Loopback only.
async fn host_post(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(request): Json<HostRequest>,
) -> Response {
    if !host::is_loopback_peer(&peer) {
        tracing::warn!("refusing POST /api/host from non-loopback peer {peer}");
        return StatusCode::FORBIDDEN.into_response();
    }
    let status = if request.enable {
        // The hosting listener serves a fresh copy of the same router and the same game.
        let app = router(state.game_tx.clone(), state.web_dir.clone(), Arc::clone(&state.host));
        state.host.start_hosting(app).await
    } else {
        state.host.stop_hosting().await
    };
    tracing::info!(
        hosting = status.hosting,
        port = status.port,
        "host status: {}",
        status.message
    );
    Json(status).into_response()
}

async fn handle_socket(socket: WebSocket, game_tx: mpsc::Sender<GameEvent>) {
    let id: ConnId = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(OUTBOX_CAPACITY);

    if game_tx.send(GameEvent::Connected { id, out: out_tx }).await.is_err() {
        return;
    }

    // Writer: drains this connection's bounded outbox. Ends when the game drops the sender.
    let writer = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    // Reader: decode at the boundary, forward to the game task.
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Binary(bytes) => match protocol::decode_client_msg(&bytes) {
                Ok(m) => {
                    if game_tx.send(GameEvent::Message { id, msg: m }).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(conn = id, "dropping connection: bad message: {e}");
                    break;
                }
            },
            Message::Close(_) => break,
            // Pings are answered by axum; text frames are not part of the protocol.
            Message::Text(_) => {
                tracing::warn!(conn = id, "dropping connection: text frame");
                break;
            }
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }
    let _ = game_tx.send(GameEvent::Disconnected { id }).await;
    // The game task drops our outbox sender on Disconnected, which ends the writer.
    let _ = writer.await;
}
