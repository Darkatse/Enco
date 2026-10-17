//! DeepSeek completions through the shared Chat Completions wire protocol.
use bindings::exports::enco::plugin::{completion, lifecycle};
use enco_sdk::protocol::{Completion, Request, Settings, types::Failure};

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
                origin: None,
                header: "authorization".into(),
                prefix: "Bearer ".into(),
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
        enco_sdk::protocol::complete(settings, request).await
    }
}
