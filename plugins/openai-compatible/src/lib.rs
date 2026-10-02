//! OpenAI-compatible completions and embeddings through the shared WIT protocol.
use bindings::exports::enco::plugin::{completion, embedding, lifecycle};
use provider_protocol::{Completion, Request, Settings, types::Failure};

mod bindings {
    #![expect(
        unsafe_code,
        reason = "the export macro expands to the generated canonical ABI glue"
    )]
    wit_bindgen::generate!({
        path: "../../wit",
        world: "provider-plugin",
        with: {
            "enco:plugin/host@0.2.0": provider_protocol::host,
            "enco:plugin/types@0.2.0": provider_protocol::types,
            "enco:plugin/completion@0.2.0/request": provider_protocol::Request,
            "enco:plugin/completion@0.2.0/completion": provider_protocol::Completion,
            "enco:plugin/completion@0.2.0/usage": provider_protocol::Usage,
            "enco:plugin/completion@0.2.0/stop-reason": provider_protocol::StopReason,
        },
    });
    use super::Plugin;
    export!(Plugin with_types_in self);
}

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(_config: String) -> Result<lifecycle::Description, Failure> {
        Ok(lifecycle::Description {
            summary: "OpenAI-compatible Chat Completions and Embeddings".into(),
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl completion::Guest for Plugin {
    async fn complete(settings: Settings, request: Request) -> Result<Completion, Failure> {
        provider_protocol::complete(settings, request).await
    }
}

impl embedding::Guest for Plugin {
    async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        provider_protocol::embed(settings, inputs).await
    }
}
