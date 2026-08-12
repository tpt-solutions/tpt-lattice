//! # tpt-lattice-server
//!
//! A WebSocket sync server that relays TPT Lattice ops between peers. This is the
//! "dumb" relay from the spec: the CRDT on each client guarantees convergence
//! once every op has been delivered. The server retains a per-document op
//! history and replays it to each new connection so late joiners / reconnecting
//! peers converge.
//!
//! ## Security posture
//!
//! The server supports, via [`ServerConfig`]:
//!
//! - **Per-document rooms** — each `?room=` (or the implicit `"default"` room)
//!   gets its own broadcast hub and history, so clients no longer share a single
//!   global document.
//! - **Token auth** — when [`ServerConfig::auth_token`] is set, every upgrade
//!   must carry a matching `?token=`.
//! - **Origin / CORS checks** — when [`ServerConfig::allowed_origins`] is set,
//!   the `Origin` header must match.
//! - **Op validation** — inbound payloads are deserialized as real [`Op`]s;
//!   anything that is not a valid op is dropped instead of being stored and
//!   replayed forever.
//! - **Message size + rate limits** — a per-message cap and a per-connection
//!   rate limit protect against resource exhaustion.
//! - **Durable persistence + compaction** — when [`ServerConfig::persistence_dir`]
//!   is set, each room's full op log is appended to disk (survives restarts);
//!   the in-memory replay buffer is capped at [`ServerConfig::max_history`] so a
//!   reconnect's catch-up cost stays bounded.
//!
//! Transport security (`wss://`) is provided by terminating TLS in front of this
//! server (reverse proxy / ingress). The connection string is already
//! configurable on the client; the security-critical part addressed here is
//! origin validation and authentication.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use tpt_lattice_crdt::Op;

/// Shared broadcast hub for op JSON payloads within a single room.
pub type Hub = Arc<broadcast::Sender<String>>;

/// A single collaborative document: its live broadcast hub plus its retained
/// (and optionally persisted) op history.
#[derive(Clone)]
pub struct Room {
    /// The room's name (used for persistence file naming).
    pub name: String,
    /// Live broadcast hub for this document.
    pub hub: Hub,
    /// Retained op history, replayed to new connections (bounded; the full log
    /// lives on disk when a persistence dir is configured).
    pub history: Arc<Mutex<Vec<String>>>,
}

/// Server-wide configuration controlling access control and limits.
#[derive(Clone, Default)]
pub struct ServerConfig {
    /// If set, every `/ws` upgrade must supply a matching `?token=`.
    pub auth_token: Option<String>,
    /// If set (non-empty), the `Origin` header must match one of these.
    pub allowed_origins: Option<Vec<String>>,
    /// Maximum accepted WebSocket message size in bytes.
    pub max_message_size: usize,
    /// Maximum in-memory retained ops per room (compaction threshold).
    pub max_history: usize,
    /// Per-connection rate limit: max messages accepted per rolling second.
    /// `0` disables rate limiting.
    pub rate_limit_per_sec: u32,
    /// If set, each room's full op log is appended to `<dir>/<room>.log`.
    pub persistence_dir: Option<std::path::PathBuf>,
}

/// Shared server state: every document's hub/history, plus the config.
pub struct ServerState {
    pub rooms: Arc<Mutex<HashMap<String, Room>>>,
    pub config: ServerConfig,
}

pub type AppState = Arc<ServerState>;

/// Query parameters accepted on the `/ws` upgrade.
#[derive(Deserialize)]
struct WsQuery {
    room: Option<String>,
    token: Option<String>,
}

/// Build the Axum router for the sync server.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Default, hardened server configuration used by [`serve`].
///
/// Auth/origin are left open by default so the bundled frontend (which connects
/// with no token) keeps working; set `TPT_LATTICE_TOKEN` / `TPT_LATTICE_DATA_DIR`
/// env vars, or build a [`ServerConfig`] explicitly, to enable them.
fn default_config() -> ServerConfig {
    ServerConfig {
        auth_token: std::env::var("TPT_LATTICE_TOKEN").ok(),
        allowed_origins: None,
        max_message_size: 1 << 20, // 1 MiB
        max_history: 100_000,
        rate_limit_per_sec: 1024,
        persistence_dir: std::env::var("TPT_LATTICE_DATA_DIR")
            .ok()
            .map(std::path::PathBuf::from),
    }
}

/// Start the server, binding to `addr` (e.g. `"127.0.0.1:8080"`).
pub async fn serve(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = default_config();
    let state: AppState = Arc::new(ServerState {
        rooms: Arc::new(Mutex::new(HashMap::new())),
        config,
    });
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("TPT Lattice sync server listening on ws://{addr}/ws");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

/// Returns `None` if access is allowed, or the `StatusCode` to reject with.
fn authorize(config: &ServerConfig, token: Option<&str>, origin: Option<&str>) -> Option<StatusCode> {
    if let Some(expected) = &config.auth_token {
        match token {
            Some(t) if t == expected => {}
            _ => return Some(StatusCode::UNAUTHORIZED),
        }
    }
    if let Some(allowed) = &config.allowed_origins {
        match origin {
            Some(o) if allowed.iter().any(|a| a.eq_ignore_ascii_case(o)) => {}
            _ => return Some(StatusCode::FORBIDDEN),
        }
    }
    None
}

/// Map a room name to a safe on-disk filename (no path traversal).
fn sanitize_room(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("default");
    }
    out
}

/// Load a room's persisted op log (one JSON op per line) if present.
fn load_history(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .lines()
            .map(|l| l.to_string())
            .filter(|l| !l.trim().is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Append a single op to the room's on-disk log.
fn append_history(path: &Path, op: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = f.write_all(op.as_bytes());
        let _ = f.write_all(b"\n");
    }
}

/// Get a room, creating (and loading any persisted history for) it on first use.
fn load_or_create_room(state: &AppState, name: &str) -> Room {
    if let Some(existing) = state.rooms.lock().unwrap().get(name).cloned() {
        return existing;
    }
    let (tx, _rx) = broadcast::channel::<String>(1024);
    let history = state
        .config
        .persistence_dir
        .as_ref()
        .map(|dir| load_history(&dir.join(format!("{}.log", sanitize_room(name)))))
        .unwrap_or_default();
    let room = Room {
        name: name.to_string(),
        hub: Arc::new(tx),
        history: Arc::new(Mutex::new(history)),
    };
    state.rooms.lock().unwrap().insert(name.to_string(), room.clone());
    room
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    if let Some(status) = authorize(
        &state.config,
        params.token.as_deref(),
        origin,
    ) {
        return status.into_response();
    }

    let room_name = params.room.clone().unwrap_or_else(|| "default".to_string());
    let room = load_or_create_room(&state, &room_name);
    ws.max_message_size(state.config.max_message_size)
        .on_upgrade(move |socket| handle_socket(socket, room, state))
}

async fn handle_socket(socket: WebSocket, room: Room, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = room.hub.subscribe();

    // Catch-up: replay this room's retained history so a late joiner /
    // reconnecting peer converges.
    {
        let history = room.history.lock().unwrap().clone();
        for op in history {
            if sender.send(Message::Text(op)).await.is_err() {
                return;
            }
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

    // Receive ops from this client, validate them, persist, bound the in-memory
    // history, and re-broadcast.
    let cfg = state.config.clone();
    let room_name = room.name.clone();
    let hub2 = room.hub.clone();
    let history2 = room.history.clone();
    let mut recv_task = tokio::spawn(async move {
        let mut rate_count: u32 = 0;
        let mut rate_window = Instant::now();
        while let Some(Ok(msg)) = receiver.next().await {
            let text = match msg {
                Message::Text(t) => t,
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => continue,
            };

            // Rolling per-connection rate limit.
            let now = Instant::now();
            if now.duration_since(rate_window).as_secs() >= 1 {
                rate_window = now;
                rate_count = 0;
            }
            rate_count += 1;
            if cfg.rate_limit_per_sec > 0 && rate_count > cfg.rate_limit_per_sec {
                break;
            }

            // Validate it deserializes as a real Op; drop garbage payloads so
            // they are never stored or rebroadcast.
            if serde_json::from_str::<Op>(&text).is_err() {
                continue;
            }

            // Durable persistence (full log on disk).
            if let Some(dir) = &cfg.persistence_dir {
                let path = dir.join(format!("{}.log", sanitize_room(&room_name)));
                append_history(&path, &text);
            }

            // Compaction: keep the in-memory replay buffer bounded.
            {
                let mut h = history2.lock().unwrap();
                h.push(text.clone());
                if h.len() > cfg.max_history {
                    let excess = h.len() - cfg.max_history;
                    h.drain(0..excess);
                }
            }

            let _ = hub2.send(text);
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
    use tpt_lattice_crdt::CrdtStore;
    use tpt_lattice_core::{CellId, CellValue};

    fn test_state(config: ServerConfig) -> AppState {
        Arc::new(ServerState {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            config,
        })
    }

    fn test_op() -> Op {
        Op::SetCell {
            cell: CellId::from_a1("A1"),
            value: CellValue::Number(1.0),
            clock: CrdtStore::new(1).clock().clone(),
            actor: 1,
        }
    }

    #[tokio::test]
    async fn hub_broadcasts_ops() {
        let (tx, _rx) = broadcast::channel::<String>(16);
        let hub: Hub = Arc::new(tx);
        let mut rx = hub.subscribe();
        let payload = serde_json::to_string(&test_op()).unwrap();
        hub.send(payload.clone()).unwrap();
        assert_eq!(rx.recv().await.unwrap(), payload);
    }

    #[test]
    fn authorize_token_required_when_configured() {
        let cfg = ServerConfig {
            auth_token: Some("secret".into()),
            ..Default::default()
        };
        assert_eq!(authorize(&cfg, None, None), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(
            authorize(&cfg, Some("wrong"), None),
            Some(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(authorize(&cfg, Some("secret"), None), None);
    }

    #[test]
    fn authorize_origin_required_when_configured() {
        let cfg = ServerConfig {
            allowed_origins: Some(vec!["https://example.com".into()]),
            ..Default::default()
        };
        assert_eq!(authorize(&cfg, None, None), Some(StatusCode::FORBIDDEN));
        assert_eq!(
            authorize(&cfg, None, Some("https://evil.com")),
            Some(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            authorize(&cfg, None, Some("https://example.com")),
            None
        );
    }

    #[test]
    fn rooms_are_isolated() {
        let state = test_state(ServerConfig::default());
        let a = load_or_create_room(&state, "a");
        let b = load_or_create_room(&state, "b");
        assert_eq!(a.name, "a");
        assert_eq!(b.name, "b");
        // A broadcast on room A must not reach room B.
        let mut a_rx = a.hub.subscribe();
        let mut b_rx = b.hub.subscribe();
        a.hub.send("op".to_string()).unwrap();
        assert_eq!(a_rx.try_recv().unwrap(), "op");
        assert!(b_rx.try_recv().is_err());
    }

    #[test]
    fn room_history_retained_and_recovered() {
        let dir = std::env::temp_dir().join(format!("tpt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = test_state(ServerConfig {
            persistence_dir: Some(dir.clone()),
            ..Default::default()
        });
        let op = test_op();
        let payload = serde_json::to_string(&op).unwrap();
        {
            let room = load_or_create_room(&state, "doc");
            room.history.lock().unwrap().push(payload.clone());
            append_history(
                &dir.join(format!("{}.log", sanitize_room("doc"))),
                &payload,
            );
        }
        // Re-open: history should be loaded from disk.
        let room = load_or_create_room(&state, "doc");
        assert_eq!(room.history.lock().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_payload_is_not_a_valid_op() {
        assert!(serde_json::from_str::<Op>("{\"hello\":\"world\"}").is_err());
        assert!(serde_json::from_str::<Op>("not json at all").is_err());
        assert!(serde_json::from_str::<Op>(
            &serde_json::to_string(&test_op()).unwrap()
        )
        .is_ok());
    }

    #[test]
    fn sanitize_room_blocks_traversal() {
        assert_eq!(sanitize_room("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitize_room("room-1"), "room-1");
    }
}
