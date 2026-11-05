//! DeepSeek completions through the SDK's shared Chat Completions mapping.
use bindings::exports::enco::plugin::{completion, lifecycle};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpRequest, HttpResponse, Request, Settings, chat, types::Failure,
};

enco_sdk::export!(completion, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "DeepSeek Chat Completions".into(),
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
        chat::encode(&settings, request)
    }

    fn decode(response: HttpResponse) -> Result<Completion, CompleteError> {
        chat::decode(&response)
    }
}
