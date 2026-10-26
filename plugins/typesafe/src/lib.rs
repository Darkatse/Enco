//! TypeSafe decisions through the shared WIT contract.
mod protocol;

use bindings::exports::enco::plugin::{decision, lifecycle};
use enco_sdk::protocol::{Settings, types::Failure};

enco_sdk::export!(decision, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "TypeSafe decisions over shared text evidence".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "api-key".into(),
                origin: None,
                placement: lifecycle::Placement::Header(lifecycle::HeaderPlacement {
                    name: "authorization".into(),
                    prefix: "Bearer ".into(),
                }),
                required: true,
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl decision::Guest for Plugin {
    async fn decide(
        settings: Settings,
        state: String,
        questions: Vec<decision::Question>,
    ) -> Result<Vec<decision::Answer>, Failure> {
        protocol::decide(settings, state, questions).await
    }
}
