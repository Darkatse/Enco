//! WIT adapters for the Runtime, Provider and Embedding ports. No provider-specific business logic lives here.
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
mod plugin;
mod runtime;
pub use runtime::{WasmError, WasmRuntime};
