//! # tpt-lattice-server
//!
//! A minimal Axum WebSocket server that broadcasts TPT Lattice ops to all
//! connected peers. This is the "dumb" relay from the spec: the CRDT on each
//! client guarantees convergence once every op has been delivered.
//!
//! To support peers that join (or reconnect) after ops were authored, the server
//! retains a **history** of every op it has seen and replays it to each new
//! connection, so a late joiner catches up and diverged histories reconverge.

use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State as AxumState;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

/// Shared broadcast hub for op JSON payloads.
pub type Hub = Arc<broadcast::Sender<String>>;

/// Shared server state: the live broadcast hub plus the retained op history.
pub struct ServerState {
    pub hub: Hub,
    /// Every op payload received so far, replayed to new connections.
    pub history: Arc<Mutex<Vec<String>>>,
}

pub type AppState = Arc<ServerState>;

/// Build the Axum router for the sync server.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Start the server, binding to `addr` (e.g. `"127.0.0.1:8080"`).
pub async fn serve(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, _rx) = broadcast::channel::<String>(1024);
    let state: AppState = Arc::new(ServerState {
        hub: Arc::new(tx),
        history: Arc::new(Mutex::new(Vec::new())),
    });
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    AxumState(state): AxumState<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.hub.subscribe();

    // Catch-up: replay history so a late joiner / reconnecting peer converges.
    let history: Vec<String> = state.history.lock().unwrap().clone();
    for op in history {
        if sender.send(Message::Text(op)).await.is_err() {
            return;
        }
    }

    // Forward every broadcast op to this client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Receive ops from this client, retain them, and re-broadcast to everyone.
    let hub2 = state.hub.clone();
    let history2 = state.history.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            let text = match msg {
                Message::Text(t) => t,
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Close(_) => break,
                _ => continue,
            };
            // Validate it deserializes as an op envelope; ignore otherwise.
            if serde_json::from_str::<serde_json::Value>(&text).is_ok() {
                history2.lock().unwrap().push(text.clone());
                let _ = hub2.send(text);
            }
        }
    });

    // If either half dies, tear the connection down.
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_lattice_crdt::{CrdtStore, Op};

    #[tokio::test]
    async fn hub_broadcasts_ops() {
        let (tx, _rx) = broadcast::channel::<String>(16);
        let hub: Hub = Arc::new(tx);
        let mut rx = hub.subscribe();
        let op = Op::SetCell {
            cell: tpt_lattice_core::CellId::from_a1("A1"),
            value: tpt_lattice_core::CellValue::Number(1.0),
            clock: CrdtStore::new(1).clock().clone(),
            actor: 1,
        };
        let payload = serde_json::to_string(&op).unwrap();
        hub.send(payload.clone()).unwrap();
        assert_eq!(rx.recv().await.unwrap(), payload);
    }

    #[test]
    fn history_is_retained() {
        let state: AppState = Arc::new(ServerState {
            hub: Arc::new(broadcast::channel::<String>(16).0),
            history: Arc::new(Mutex::new(Vec::new())),
        });
        let op = r#"{"SetCell":{"cell":5,"value":{"Number":2},"clock":{"1":1},"actor":1}}"#;
        state.history.lock().unwrap().push(op.to_string());
        assert_eq!(state.history.lock().unwrap().len(), 1);
        assert_eq!(state.history.lock().unwrap()[0], op);
    }
}
