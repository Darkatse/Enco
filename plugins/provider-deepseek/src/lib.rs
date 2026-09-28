//! DeepSeek adapter. Reasoning extensions and cache usage remain in the shared wire path.
use bindings::exports::enco::plugin::{lifecycle, provider};
use provider_protocol::{Completion, Request, Settings, bindings, types::Failure};
const PROVIDER: &str = "deepseek";

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(_config: String) -> lifecycle::PluginInfo {
        lifecycle::PluginInfo {
            name: PROVIDER.into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

impl provider::Guest for Plugin {
    async fn complete(settings: Settings, request: Request) -> Result<Completion, Failure> {
        provider_protocol::complete(settings, request, PROVIDER).await
    }

    async fn embed(_settings: Settings, _inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        Err(provider_protocol::failure(
            "provider.bad_request",
            "DeepSeek does not provide embeddings; configure a separate embedding provider",
            false,
        ))
    }
}
bindings::export!(Plugin with_types_in bindings);
