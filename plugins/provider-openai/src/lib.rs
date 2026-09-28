//! OpenAI-compatible completions and embeddings through the shared WIT protocol.
use bindings::exports::enco::plugin::{lifecycle, provider};
use provider_protocol::{Completion, Request, Settings, bindings, types::Failure};
const PROVIDER: &str = "openai-compatible";

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

    async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        provider_protocol::embed(settings, inputs).await
    }
}
bindings::export!(Plugin with_types_in bindings);
