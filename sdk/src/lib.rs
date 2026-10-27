//! Rust bindings, world exports, and shared schema, protocol and channel helpers for plugins.
//!
//! The host writes this crate to `sdk/` in every home, replacing any edits there; plugins depend on
//! it by path.
#![warn(missing_docs)]
pub mod channel;
mod http;
pub mod protocol;
mod schema;
use base64::{Engine, engine::general_purpose::STANDARD};
pub use wit_bindgen;

/// Types and host functions generated from the WIT.
///
/// Plugins built with [`export!`] use these same types, so values pass between a plugin and
/// [`protocol`] or [`channel`] as they are.
#[expect(
    missing_docs,
    reason = "wit-bindgen adds undocumented items such as package modules; the contract gate checks the WIT"
)]
pub mod bindings {
    // One world for every interface the helpers use: a second `generate!` would define the
    // shared export macros twice. Plugins export their own worlds through `export!`.
    wit_bindgen::generate!({
        path: "wit",
        inline: "
            package enco:sdk;
            world sdk {
                include enco:plugin/completion-plugin@0.9.0;
                export enco:plugin/channel@0.9.0;
            }
        ",
        generate_all,
        pub_export_macro: true,
        additional_derives: [PartialEq],
    });
}
pub use bindings::enco::plugin::{host, types};

/// Parse normalized composer options into the type that generated their schema.
///
/// The host has already checked the options; custom deserialization failures are `compose.failed`.
pub fn parse_options<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, types::Failure> {
    serde_json::from_str(input)
        .map_err(|error| protocol::failure("compose.failed", error.to_string(), false))
}

/// The lowercase BLAKE3 digest of source text before the composer formats it.
pub fn source_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

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

/// The canonical schema of `T`, used by tool arguments and composer options.
///
/// Return it as JSON text from a tool's [`types::ToolSpec`] or a composer's `options-schema`.
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
/// embedding), `decision`, `tools`, `composer` or `channel`.
///
/// Call it in a crate in `plugins/<name>` that depends on this SDK as `enco_sdk`. It defines a
/// `bindings` module beside the call, whose `exports` hold the `Guest` traits to implement; the
/// WIT is read from `../../sdk/wit`. The traits take the SDK's own types where it has helpers for
/// them, such as [`protocol::Request`] and [`channel::ActivityView`]; name those from the SDK, not
/// from `bindings`.
#[macro_export]
macro_rules! export {
    (completion, $implementation:ident) => { $crate::export!(@provider "completion-plugin", $implementation); };
    (provider, $implementation:ident) => { $crate::export!(@provider "provider-plugin", $implementation); };
    (decision, $implementation:ident) => { $crate::export!(@world "decision-plugin", $implementation, [],); };
    (tools, $implementation:ident) => { $crate::export!(@world "tool-plugin", $implementation, [],); };
    (composer, $implementation:ident) => { $crate::export!(@world "composer-plugin", $implementation, [PartialEq],); };
    (channel, $implementation:ident) => {
        $crate::export!(@bindings "channel-plugin", $implementation, [PartialEq],
            "enco:plugin/channel@0.9.0/activity-view": enco_sdk::channel::ActivityView,
            "enco:plugin/channel@0.9.0/tool-activity": enco_sdk::channel::ToolActivity,
            "enco:plugin/channel@0.9.0/tool-state": enco_sdk::channel::ToolState,
        );
    };
    (@provider $world:literal, $implementation:ident) => {
        $crate::export!(@world $world, $implementation, [],
            "enco:plugin/completion@0.9.0/request": enco_sdk::protocol::Request,
            "enco:plugin/completion@0.9.0/completion": enco_sdk::protocol::Completion,
            "enco:plugin/completion@0.9.0/usage": enco_sdk::protocol::Usage,
            "enco:plugin/completion@0.9.0/stop-reason": enco_sdk::protocol::StopReason,
        );
    };
    (@world $world:literal, $implementation:ident, [$($derives:path),*], $($types:tt)*) => {
        $crate::export!(@bindings $world, $implementation, [$($derives),*],
            "enco:plugin/host@0.9.0": enco_sdk::host,
            $($types)*
        );
    };
    (@bindings $world:literal, $implementation:ident, [$($derives:path),*], $($types:tt)*) => {
        // The contract's own documentation covers these bindings, so a plugin's manual omits them.
        #[doc(hidden)]
        mod bindings {
            enco_sdk::wit_bindgen::generate!({
                path: "../../sdk/wit",
                world: $world,
                runtime_path: "enco_sdk::wit_bindgen::rt",
                additional_derives: [$($derives),*],
                with: {
                    "enco:plugin/types@0.9.0": enco_sdk::types,
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
