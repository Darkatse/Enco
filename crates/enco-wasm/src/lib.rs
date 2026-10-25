//! Runs plugin components with Wasmtime and adapts their WIT exports to the kernel's ports.
//!
//! Nothing here depends on which plugin or service is behind a component.
mod bindings;

mod convert;
mod credentials;
mod engine;
mod host_imports;
mod limits;
mod plugin;
mod runtime;
mod tools;
pub use runtime::{WasmError, WasmRuntime};
