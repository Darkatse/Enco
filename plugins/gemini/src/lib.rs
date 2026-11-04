//! Gemini Generate Content and batch embeddings through the shared transport.
mod request;
mod response;
use bindings::exports::enco::plugin::{completion, embedding, lifecycle};
use enco_sdk::protocol::{
    CompleteError, Completion, Overflow, Request, Settings, complete_error, number_after,
    types::Failure,
};
use serde_json::Value;

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
                placements: vec![lifecycle::Placement {
                    origin: None,
                    location: lifecycle::Location::Header(lifecycle::HeaderPlacement {
                        name: "x-goog-api-key".into(),
                        prefix: "".into(),
                    }),
                }],
                required: true,
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl completion::Guest for Plugin {
    async fn complete(settings: Settings, request: Request) -> Result<Completion, CompleteError> {
        let body = request::completion(&settings, request).map_err(CompleteError::Failed)?;
        let path = format!("models/{}:generateContent", settings.model);
        let response = enco_sdk::protocol::post(&settings, &path, body, None)
            .await
            .map_err(|error| complete_error(error, overflow))?;
        response::completion(response).map_err(CompleteError::Failed)
    }
}

/// Gemini's refusal of an over-long request states only its limit: "The input token count
/// exceeds the maximum number of tokens allowed N." A proxy may wrap the answer in its own
/// envelope but keeps the sentence, so the sentence is what identifies it.
fn overflow(answer: &Value) -> Option<Overflow> {
    let message = answer.pointer("/error/message")?.as_str()?;
    let accepted = number_after(
        message,
        "The input token count exceeds the maximum number of tokens allowed ",
    )?;
    Some(Overflow {
        counted: None,
        accepted: Some(accepted),
        message: message.into(),
    })
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
