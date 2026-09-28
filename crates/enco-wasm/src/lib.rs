//! WIT adapters for the Provider port. No provider-specific business logic lives here.
mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "provider-plugin",
        exports: { default: async },
    });
}

mod convert;
mod engine;
mod host_imports;
mod limits;
mod provider;
pub use engine::WasmEngine;
pub use provider::{ProviderSettings, WasmError, WasmProvider};
