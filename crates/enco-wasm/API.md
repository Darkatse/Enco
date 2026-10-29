<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_wasm`

`src/lib.rs`

Runs plugin components with Wasmtime and adapts their WIT exports to the kernel's ports.

Nothing here depends on which plugin or service is behind a component.

- enum `WasmError` — Why the plugin runtime could not start.
- struct `WasmRuntime` — The host's plugin runtime: one engine and one HTTP client shared by every loaded plugin.
- fn `plugin_limits` — The limits plugins run within, each as a description and its limit, for the manual.

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

Implements:

```rust
impl Debug for WasmError
impl Display for WasmError
impl Error for WasmError
impl From<Error> for WasmError
```

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

Implements:

```rust
impl Runtime for WasmRuntime
```

## fn `plugin_limits`

`src/limits.rs`

```rust
pub fn plugin_limits() -> Vec<(String, String)>
```

The limits plugins run within, each as a description and its limit, for the manual.
