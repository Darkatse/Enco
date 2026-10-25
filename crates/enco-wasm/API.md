<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_wasm`

`src/lib.rs`

Runs plugin components with Wasmtime and adapts their WIT exports to the kernel's ports.

Nothing here depends on which plugin or service is behind a component.

- enum `WasmError` — Why the plugin runtime could not start.
- struct `WasmRuntime` — The host's plugin runtime: one engine and one HTTP client shared by every loaded plugin.

## enum `WasmError`

`src/runtime.rs`

```rust
pub enum WasmError {
    Engine(wasmtime::Error),
    Clock(std::io::Error),
    Http(reqwest::Error),
}
```

Why the plugin runtime could not start.

- `Engine` — Wasmtime could not create the engine or its compiled-code cache.
- `Clock` — The engine's epoch clock thread could not start.
- `Http` — The HTTP client could not be built.

Implements `Debug`, `Display`, `Error`, `From<Error>`.

## struct `WasmRuntime`

`src/runtime.rs`

```rust
pub struct WasmRuntime {
    /* private fields */
}
```

The host's plugin runtime: one engine and one HTTP client shared by every loaded plugin.

A loaded plugin keeps no endpoint or credential value; both come with each call.

### `WasmRuntime::new`

```rust
pub fn new() -> Result<Self, WasmError>
```

Start the shared engine and its epoch clock thread, and build the HTTP client.

Implements `Runtime`.
