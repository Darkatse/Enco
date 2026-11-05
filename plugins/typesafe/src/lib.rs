//! Maps decisions to TypeSafe's `/systemone` and its answers back.
mod protocol;

use bindings::exports::enco::plugin::{decision, lifecycle};
use enco_sdk::protocol::{HttpRequest, HttpResponse, Settings, types::Failure};

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

impl decision::Guest for Plugin {
    fn encode(
        settings: Settings,
        state: String,
        questions: Vec<decision::Question>,
    ) -> Result<HttpRequest, Failure> {
        protocol::encode(&settings, &state, &questions)
    }

    fn decode(
        questions: Vec<decision::Question>,
        response: HttpResponse,
    ) -> Result<Vec<decision::Answer>, Failure> {
        protocol::decode(&questions, &response)
    }
}
