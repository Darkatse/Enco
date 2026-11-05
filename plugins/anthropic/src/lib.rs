//! Anthropic Messages completions, as pure encode and decode.
mod request;
mod response;
mod stream;
use bindings::exports::enco::plugin::{completion, lifecycle};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpRequest, HttpResponse, Overflow, Request, Settings, answer,
    stream::post_completion, types::Failure,
};
use serde_json::Value;

/// The API version every request names. Later versions only tighten checks on fields Enco does
/// not send, and some hosted deployments accept only this one for older models.
const VERSION: &str = "2023-06-01";

enco_sdk::export!(completion, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "Anthropic Messages".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "api-key".into(),
                placements: vec![lifecycle::Placement {
                    origin: None,
                    location: lifecycle::Location::Header(lifecycle::HeaderPlacement {
                        name: "x-api-key".into(),
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
        let (body, stream) = request::completion(&settings, request)?;
        let mut http = post_completion("/messages", &body, stream);
        http.headers
            .push(("anthropic-version".into(), VERSION.into()));
        Ok(http)
    }

    fn decode(response: HttpResponse) -> Result<Completion, CompleteError> {
        let response = answer(&response, overflow, stream::fold)?;
        response::completion(&response).map_err(CompleteError::Failed)
    }
}

/// Anthropic's refusal of an over-long request: "prompt is too long: N tokens > M maximum".
///
/// The limit counts the input only, since the output does not count against it, so M is the
/// input it accepts. Only that whole sentence identifies the refusal.
fn overflow(answer: &Value) -> Option<Overflow> {
    let message = answer.pointer("/error/message")?.as_str()?;
    let (counted, accepted) = message
        .strip_prefix("prompt is too long: ")?
        .strip_suffix(" maximum")?
        .split_once(" tokens > ")?;
    Some(Overflow {
        counted: Some(counted.parse().ok()?),
        accepted: Some(accepted.parse().ok()?),
        message: message.into(),
    })
}

#[cfg(test)]
mod tests;
