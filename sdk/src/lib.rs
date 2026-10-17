//! Plugin-side WIT bindings and shared protocol code, distributed with the host.
mod http;
pub mod protocol;
pub use wit_bindgen;

/// Shared type identity for the protocol helpers and plugin exports.
pub mod bindings {
    wit_bindgen::generate!({ path: "wit", world: "completion-plugin", pub_export_macro: true });
}
pub use bindings::enco::plugin::{host, types};

/// Generate the chosen world and export its implementation from a plugin in `plugins/<name>`.
#[macro_export]
macro_rules! export {
    (completion, $implementation:ident) => { $crate::export!(@provider "completion-plugin", $implementation); };
    (provider, $implementation:ident) => { $crate::export!(@provider "provider-plugin", $implementation); };
    (decision, $implementation:ident) => { $crate::export!(@world "decision-plugin", $implementation,); };
    (tools, $implementation:ident) => { $crate::export!(@world "tool-plugin", $implementation,); };
    (@provider $world:literal, $implementation:ident) => {
        $crate::export!(@world $world, $implementation,
            "enco:plugin/completion@0.4.0/request": enco_sdk::protocol::Request,
            "enco:plugin/completion@0.4.0/completion": enco_sdk::protocol::Completion,
            "enco:plugin/completion@0.4.0/usage": enco_sdk::protocol::Usage,
            "enco:plugin/completion@0.4.0/stop-reason": enco_sdk::protocol::StopReason,
        );
    };
    (@world $world:literal, $implementation:ident, $($types:tt)*) => {
        mod bindings {
            enco_sdk::wit_bindgen::generate!({
                path: "../../sdk/wit",
                world: $world,
                runtime_path: "enco_sdk::wit_bindgen::rt",
                with: {
                    "enco:plugin/host@0.4.0": enco_sdk::host,
                    "enco:plugin/types@0.4.0": enco_sdk::types,
                    $($types)*
                },
            });
            #[expect(unsafe_code, reason = "the export macro expands to generated canonical ABI glue")]
            mod entry {
                use super::super::$implementation;
                super::export!($implementation with_types_in super);
            }
        }
    };
}
