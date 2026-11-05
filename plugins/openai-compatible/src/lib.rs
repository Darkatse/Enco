//! OpenAI-compatible Chat Completions and embeddings, mapped by the SDK's shared implementation.
use bindings::exports::enco::plugin::{completion, embedding, lifecycle};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpRequest, HttpResponse, Request, Settings, chat, types::Failure,
};

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
                placements: vec![lifecycle::Placement {
                    origin: None,
                    location: lifecycle::Location::Header(lifecycle::HeaderPlacement {
                        name: "authorization".into(),
                        prefix: "Bearer ".into(),
                    }),
                }],
                required: false,
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl completion::Guest for Plugin {
    fn encode(settings: Settings, request: Request) -> Result<HttpRequest, Failure> {
        chat::encode(&settings, request)
    }

    fn decode(response: HttpResponse) -> Result<Completion, CompleteError> {
        chat::decode(&response)
    }
}

impl embedding::Guest for Plugin {
    fn encode(settings: Settings, inputs: Vec<String>) -> Result<HttpRequest, Failure> {
        chat::encode_embedding(&settings, &inputs)
    }

    fn decode(count: u32, response: HttpResponse) -> Result<Vec<Vec<f32>>, Failure> {
        chat::decode_embedding(count, &response)
    }
}
