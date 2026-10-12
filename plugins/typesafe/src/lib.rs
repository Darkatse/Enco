//! TypeSafe decisions through the shared WIT contract.
mod protocol;

use bindings::exports::enco::plugin::{decision, lifecycle};
use provider_protocol::{Settings, types::Failure};

mod bindings {
    #![expect(
        unsafe_code,
        reason = "the export macro expands to the generated canonical ABI glue"
    )]
    wit_bindgen::generate!({
        path: "../../wit",
        world: "decision-plugin",
        with: {
            "enco:plugin/host@0.3.0": provider_protocol::host,
            "enco:plugin/types@0.3.0": provider_protocol::types,
        },
    });
    use super::Plugin;
    export!(Plugin with_types_in self);
}

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        provider_protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "TypeSafe decisions over shared text evidence".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credential: Some(lifecycle::Credential {
                header: "authorization".into(),
                prefix: "Bearer ".into(),
                required: true,
            }),
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
