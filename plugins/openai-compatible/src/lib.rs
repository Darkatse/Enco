//! OpenAI-compatible completions and embeddings through the shared WIT protocol.
use bindings::exports::enco::plugin::{completion, embedding, lifecycle};
use enco_sdk::protocol::{Completion, Request, Settings, types::Failure};

enco_sdk::export!(provider, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "OpenAI-compatible Chat Completions and Embeddings".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "api-key".into(),
                origin: None,
                placement: lifecycle::Placement::Header(lifecycle::HeaderPlacement {
                    name: "authorization".into(),
                    prefix: "Bearer ".into(),
                }),
                required: false,
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl completion::Guest for Plugin {
    async fn complete(settings: Settings, request: Request) -> Result<Completion, Failure> {
        enco_sdk::protocol::complete(settings, request).await
    }
}

impl embedding::Guest for Plugin {
    async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        enco_sdk::protocol::embed(settings, inputs).await
    }
}
