//! Runs plugin components with Wasmtime and adapts their WIT exports to the kernel's ports.
//!
//! Nothing here depends on which plugin or service is behind a component.
#![warn(missing_docs)]
// Generated from the WIT, whose documentation is the contract; manuals leave it out.
#[doc(hidden)]
mod bindings;

mod call_world;
mod composer;
mod composer_convert;
mod composer_events;
mod convert;
mod credentials;
mod engine;
mod host_imports;
mod limits;
mod plugin;
mod runtime;
mod tools;
pub use limits::call_limits;
pub use runtime::{WasmError, WasmRuntime};
