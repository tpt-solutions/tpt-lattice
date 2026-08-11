# tpt-lattice-server

A minimal [Axum](https://docs.rs/axum) **WebSocket** server that broadcasts
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) ops to all connected peers.

This is the "dumb relay" from the design: it does **not** reconcile histories itself. The CRDT
on each client guarantees convergence once every op has been delivered to every peer, so the
server only needs to fan out JSON op payloads.

## Features

- **WebSocket endpoint** at `/ws` using `axum` and `tokio`.
- **Broadcast hub** — every op received from one client is re-broadcast to all others via a
  `tokio::sync::broadcast` channel.
- Payload validation — inbound text is checked to deserialize as JSON before relaying.
- Per-connection task supervision — a dead sender or receiver tears the socket down cleanly.

## Installation

```toml
[dependencies]
tpt-lattice-server = "0.1.0"
```

This crate ships a binary; run it directly:

```sh
cargo run -p tpt-lattice-server
# listens on ws://127.0.0.1:8080/ws
```

## Embedding

The router and serve loop are reusable as a library:

```rust
use tpt_lattice_server::{router, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    serve("127.0.0.1:8080").await
}
```

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
