//! Rust bindings for the plugin contract, the [`export!`] macro, and helpers for plugins that call
//! JSON services over HTTP.
//!
//! The host writes this crate to `sdk/` in every home, replacing any edits there; plugins depend on
//! it by path.
mod http;
pub mod protocol;
mod schema;
use base64::{Engine, engine::general_purpose::STANDARD};
pub use wit_bindgen;

/// Types and host functions generated from the WIT.
///
/// Plugins built with [`export!`] use these same types, so a value from [`protocol`] can be
/// returned from a plugin as it is.
pub mod bindings {
    wit_bindgen::generate!({ path: "wit", world: "completion-plugin", pub_export_macro: true });
}
pub use bindings::enco::plugin::{host, types};

impl types::ImageFormat {
    /// The MIME type of this format, such as `image/png`.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

impl types::Image {
    /// The image bytes as standard Base64 with padding, as data URLs and inline JSON images expect.
    pub fn base64(&self) -> String {
        STANDARD.encode(&self.data)
    }
}

/// The canonical argument schema of `T`; a [`types::ToolSpec`] takes it as JSON text.
///
/// Field docs become the parameter descriptions the model reads. Every struct in `T` needs
/// `#[serde(deny_unknown_fields)]`, because the host accepts only closed objects. Maps, sets and
/// recursive types fall outside the subset the host accepts, so a plugin that uses them fails to
/// load.
pub fn input_schema<T: schemars::JsonSchema>() -> serde_json::Value {
    schema::canonical_schema::<T>()
}

/// Parse a tool call's `input` into `T`; a failure has code `tool.invalid_arguments` and is not
/// retryable.
///
/// The host has already checked `input` against the tool's declared schema, so this fails only on
/// rules the type enforces itself, such as a custom `Deserialize`.
pub fn parse_arguments<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, types::Failure> {
    serde_json::from_str(input)
        .map_err(|error| protocol::failure("tool.invalid_arguments", error.to_string(), false))
}

/// Export `$implementation` as the plugin for one world: `completion`, `provider` (completion and
/// embedding), `decision` or `tools`.
///
/// Call it in a crate in `plugins/<name>` that depends on this SDK as `enco_sdk`. It defines a
/// `bindings` module beside the call, whose `exports` hold the `Guest` traits to implement; the
/// WIT is read from `../../sdk/wit`.
#[macro_export]
macro_rules! export {
    (completion, $implementation:ident) => { $crate::export!(@provider "completion-plugin", $implementation); };
    (provider, $implementation:ident) => { $crate::export!(@provider "provider-plugin", $implementation); };
    (decision, $implementation:ident) => { $crate::export!(@world "decision-plugin", $implementation,); };
    (tools, $implementation:ident) => { $crate::export!(@world "tool-plugin", $implementation,); };
    (@provider $world:literal, $implementation:ident) => {
        $crate::export!(@world $world, $implementation,
            "enco:plugin/completion@0.6.0/request": enco_sdk::protocol::Request,
            "enco:plugin/completion@0.6.0/completion": enco_sdk::protocol::Completion,
            "enco:plugin/completion@0.6.0/usage": enco_sdk::protocol::Usage,
            "enco:plugin/completion@0.6.0/stop-reason": enco_sdk::protocol::StopReason,
        );
    };
    (@world $world:literal, $implementation:ident, $($types:tt)*) => {
        mod bindings {
            enco_sdk::wit_bindgen::generate!({
                path: "../../sdk/wit",
                world: $world,
                runtime_path: "enco_sdk::wit_bindgen::rt",
                with: {
                    "enco:plugin/host@0.6.0": enco_sdk::host,
                    "enco:plugin/types@0.6.0": enco_sdk::types,
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
