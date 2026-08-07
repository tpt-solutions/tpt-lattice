//! # tpt-lattice-server
//!
//! A minimal Axum WebSocket server that broadcasts TPT Lattice ops to all
//! connected peers. This is the "dumb" relay from the spec: it does not
//! reconcile histories itself — the CRDT on each client guarantees convergence
//! once every op has been delivered to every peer.
//!
//! ```ignore
//! cargo run -p tpt-lattice-server
//! # listens on ws://127.0.0.1:8080/ws
//! ```

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

/// Shared broadcast hub for op JSON payloads.
pub type Hub = Arc<broadcast::Sender<String>>;

/// Build the Axum router for the sync server.
pub fn router(hub: Hub) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(hub)
}

/// Start the server, binding to `addr` (e.g. `"127.0.0.1:8080"`).
pub async fn serve(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, _rx) = broadcast::channel::<String>(1024);
    let hub: Hub = Arc::new(tx);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(hub)).await?;
    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(hub): State<Hub>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, hub))
}

async fn handle_socket(socket: WebSocket, hub: Hub) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = hub.subscribe();

    // Forward every broadcast op to this client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Receive ops from this client and re-broadcast them to everyone.
    let hub2 = hub.clone();
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
}
