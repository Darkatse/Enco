//! Gemini Generate Content and batch embeddings through the shared transport.
mod request;
mod response;
use bindings::exports::enco::plugin::{completion, embedding, lifecycle};
use enco_sdk::protocol::{Completion, Request, Settings, types::Failure};

enco_sdk::export!(provider, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "Gemini Generate Content and Embeddings".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "api-key".into(),
                origin: None,
                placement: lifecycle::Placement::Header(lifecycle::HeaderPlacement {
                    name: "x-goog-api-key".into(),
                    prefix: "".into(),
                }),
                required: true,
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl completion::Guest for Plugin {
    async fn complete(settings: Settings, request: Request) -> Result<Completion, Failure> {
        let body = request::completion(&settings, request)?;
        let path = format!("models/{}:generateContent", settings.model);
        response::completion(enco_sdk::protocol::post(&settings, &path, body, None).await?)
    }
}

impl embedding::Guest for Plugin {
    async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        let count = inputs.len();
        let body = request::embedding(&settings, inputs)?;
        let path = format!("models/{}:batchEmbedContents", settings.model);
        response::embeddings(
            enco_sdk::protocol::post(&settings, &path, body, Some(30000)).await?,
            count,
        )
    }
}

#[cfg(test)]
mod tests;
