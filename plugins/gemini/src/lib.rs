//! Gemini Generate Content and batch embeddings, as pure encode and decode.
mod request;
mod response;
use bindings::exports::enco::plugin::{completion, embedding, lifecycle};
use enco_sdk::protocol::{
    COMPLETION_TIMEOUT_MS, CompleteError, Completion, EMBEDDING_TIMEOUT_MS, HttpRequest,
    HttpResponse, Overflow, Request, Settings, answer, embeddings, json, number_after, post_json,
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
    fn encode(settings: Settings, request: Request) -> Result<HttpRequest, Failure> {
        let body = request::completion(&settings, request)?;
        let path = format!("/models/{}:generateContent", settings.model);
        Ok(post_json(path, &body, Some(COMPLETION_TIMEOUT_MS)))
    }

    fn decode(response: HttpResponse) -> Result<Completion, CompleteError> {
        let response = answer(&response, overflow)?;
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
    fn encode(settings: Settings, inputs: Vec<String>) -> Result<HttpRequest, Failure> {
        let body = request::embedding(&settings, inputs)?;
        let path = format!("/models/{}:batchEmbedContents", settings.model);
        Ok(post_json(path, &body, Some(EMBEDDING_TIMEOUT_MS)))
    }

    fn decode(count: u32, response: HttpResponse) -> Result<Vec<Vec<f32>>, Failure> {
        embeddings(count, response::embeddings(json(&response)?)?)
    }
}

#[cfg(test)]
mod tests;
