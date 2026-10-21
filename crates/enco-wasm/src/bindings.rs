//! Each view requires only its own exports; shared interfaces keep one Rust type identity.
mod completion {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "completion-plugin",
        exports: { default: async },
    });
}

mod base {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "base",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::completion::enco::plugin::types,
            "enco:plugin/host": super::completion::enco::plugin::host,
        },
    });
}

mod embedding {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "embedding-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::completion::enco::plugin::types,
            "enco:plugin/host": super::completion::enco::plugin::host,
        },
    });
}

mod decision {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "decision-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::completion::enco::plugin::types,
            "enco:plugin/host": super::completion::enco::plugin::host,
        },
    });
}

mod tools {
    wasmtime::component::bindgen!({
        path: "../../sdk/wit",
        world: "tool-plugin",
        exports: { default: async },
        with: {
            "enco:plugin/types": super::completion::enco::plugin::types,
            "enco:plugin/host": super::completion::enco::plugin::host,
        },
    });
}

/// Package version of `sdk/wit/plugin.wit`; change it together with the WIT.
pub(crate) const CONTRACT_VERSION: &str = "0.5.0";

pub(crate) use base::BasePre;
pub(crate) use base::exports::enco::plugin::lifecycle;
pub(crate) use completion::{CompletionPluginPre, enco, exports};
pub(crate) use decision::{DecisionPluginPre, exports::enco::plugin::decision as decisions};
pub(crate) use embedding::EmbeddingPluginPre;
pub(crate) use tools::ToolPluginPre;
