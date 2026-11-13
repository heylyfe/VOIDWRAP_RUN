//! WebSocket transport. Validates framing and decodes client messages at the boundary;
//! everything past this point only sees well-formed `ClientMsg` values.

use crate::game::{ConnId, GameEvent, OUTBOX_CAPACITY};
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use protocol::MAX_CLIENT_MSG_BYTES;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub fn router(game_tx: mpsc::Sender<GameEvent>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(game_tx)
}

async fn ws_handler(ws: WebSocketUpgrade, State(game_tx): State<mpsc::Sender<GameEvent>>) -> impl IntoResponse {
    ws.max_message_size(MAX_CLIENT_MSG_BYTES as usize)
        .max_frame_size(MAX_CLIENT_MSG_BYTES as usize)
        .on_upgrade(move |socket| handle_socket(socket, game_tx))
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
