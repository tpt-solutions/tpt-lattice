# tpt-lattice-wasm

[`wasm-bindgen`](https://docs.rs/wasm-bindgen) glue exposing the
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) engine to JavaScript. The engine
runs entirely inside a **Web Worker**; this crate is the worker's API surface.

The protocol is a simple JSON request/response envelope, which keeps the main thread free and
lets the UI speak plain JSON (a binary protocol can be layered on top later).

## Protocol

The worker accepts a JSON `Request` and returns a JSON `Response`:

```text
{ "type": "SetCell",    "cell": "A1", "value": { "Number": 42 } }
{ "type": "SetFormula", "cell": "B1", "formula": "=A1 * 2" }
{ "type": "GetCell",    "cell": "B1" }            -> { "type": "Value", "value": { "Number": 84 } }
{ "type": "Evaluate" }
{ "type": "ApplyOps",   "ops": [ ... ] }          -> { "type": "OpsAccepted", "count": N }
{ "type": "Reset" }
```

## Exposed API

- **`LatticeEngine`** — the worker-side engine handle. Construct it with `new()` and drive it
  with `handle(request_json) -> response_json`. Internally it pairs an `Evaluator` with a
  `CrdtStore`, so local edits are CRDT-recorded and remote ops rebuild the evaluator.
- **`set_cells_json`** — a convenience batch API that sets many `(A1, JSON value)` pairs at once.

## Building

```sh
# Install the toolchain (one time)
cargo install wasm-pack

# Build the WebAssembly package into ./pkg
wasm-pack build crates/tpt-lattice-wasm --target web --out-dir pkg
```

The generated `pkg/` is then imported from a Web Worker (see the SolidJS frontend in Phase 4).

## Example (worker sketch)

```js
import init, { LatticeEngine } from "./pkg/tpt_lattice_wasm.js";

await init();
const engine = new LatticeEngine();
engine.handle(JSON.stringify({ type: "SetCell", cell: "A1", value: { Number: 21 } }));
engine.handle(JSON.stringify({ type: "SetFormula", cell: "B1", formula: "=A1 * 2" }));
engine.handle(JSON.stringify({ type: "Evaluate" }));
const res = engine.handle(JSON.stringify({ type: "GetCell", cell: "B1" }));
console.log(JSON.parse(res)); // { "type": "Value", "value": { "Number": 42 } }
```

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
