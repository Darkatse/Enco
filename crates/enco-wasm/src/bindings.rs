//! Wasmtime bindings: one for what the host links and the lifecycle every plugin exports, and one
//! for each other interface a plugin can export.
//!
//! A plugin that exports several interfaces, such as a provider, is bound once for each; the
//! interfaces they share map to one set of Rust types.
mod base {
    // The host links `host` for every component and calls `lifecycle` on each; no plugin world
    // pairs them, since only tool plugins import `host`.
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        inline: "
            package enco:host;
            world base {
                import enco:plugin/host@0.16.0;
                export enco:plugin/lifecycle@0.16.0;
            }
        ",
        world: "enco:host/base",
        exports: { default: async },
    });
}

mod completion {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "completion-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::base::enco::plugin::types,
        },
    });
}

mod embedding {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "embedding-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::base::enco::plugin::types,
        },
    });
}

mod decision {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "decision-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::base::enco::plugin::types,
        },
    });
}

mod tools {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "tool-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::base::enco::plugin::types,
            "enco:plugin/host": super::base::enco::plugin::host,
        },
    });
}

mod composer {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "composer-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::base::enco::plugin::types,
        },
    });
}

/// Package version of `sdk/wit/plugin.wit`; change it together with the WIT.
pub(crate) const CONTRACT_VERSION: &str = "0.16.0";

pub(crate) use base::exports::enco::plugin::lifecycle;
pub(crate) use base::{BasePre, enco};
pub(crate) use completion::{
    CompletionPluginPre, exports::enco::plugin::completion as completions,
};
pub(crate) use decision::{DecisionPluginPre, exports::enco::plugin::decision as decisions};
pub(crate) use embedding::EmbeddingPluginPre;
pub(crate) use tools::ToolPluginPre;

pub(crate) use composer::{ComposerPluginPre, exports::enco::plugin::composer as composers};

mod channel {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "channel-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::base::enco::plugin::types,
        },
    });
}

pub(crate) use channel::{ChannelPluginPre, exports::enco::plugin::channel as channels};
