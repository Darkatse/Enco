//! OpenAI Responses completions, as pure encode and decode over stateless requests.
mod request;
mod response;
mod stream;
use bindings::exports::enco::plugin::{completion, lifecycle};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpRequest, HttpResponse, Overflow, Request, Settings, answer,
    stream::post_completion, types::Failure,
};
use serde_json::Value;

enco_sdk::export!(completion, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "OpenAI Responses".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "api-key".into(),
                placements: vec![lifecycle::Placement {
                    origin: None,
                    location: lifecycle::Location::Header(lifecycle::HeaderPlacement {
                        name: "authorization".into(),
                        prefix: "Bearer ".into(),
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
        let (body, stream) = request::completion(&settings, request)?;
        Ok(post_completion("/responses", &body, stream))
    }

    fn decode(response: HttpResponse) -> Result<Completion, CompleteError> {
        let response = answer(&response, overflow, stream::fold)?;
        response::completion(&response).map_err(CompleteError::Failed)
    }
}

/// The Responses refusal of an over-long input: an error with the code `context_length_exceeded`.
///
/// The service gives no numbers, only a sentence that the input exceeds the model's window.
fn overflow(answer: &Value) -> Option<Overflow> {
    let error = answer.get("error")?;
    if error.get("code")?.as_str()? != "context_length_exceeded" {
        return None;
    }
    Some(Overflow {
        counted: None,
        accepted: None,
        message: error.get("message")?.as_str()?.into(),
    })
}

#[cfg(test)]
mod tests;
