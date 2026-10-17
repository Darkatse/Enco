//! Shared tool fixture for source registration, evaluation and cancellation.
use bindings::exports::enco::plugin::{lifecycle, tools};
use enco_sdk::types;

enco_sdk::export!(tools, Plugin);

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, types::Failure> {
        if config != "{}" {
            return Err(types::Failure {
                code: "plugin.config".into(),
                message: "This plugin has no configuration fields".into(),
                retryable: false,
            });
        }
        Ok(lifecycle::Description {
            summary: "Test tool fixture".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: Vec::new(),
        })
    }

    async fn probe() -> Result<(), types::Failure> {
        Ok(())
    }
}

impl tools::Guest for Plugin {
    fn list(_config: String) -> Vec<types::ToolSpec> {
        vec![
            types::ToolSpec {
                name: "fixture_echo".into(),
                description: "Return the supplied object unchanged.".into(),
                input_schema: r#"{"type":"object"}"#.into(),
                effect: types::Effect::ReadOnly,
            },
            types::ToolSpec {
                name: "fixture_spin".into(),
                description: "Keep computing until the invocation is cancelled.".into(),
                input_schema: r#"{"type":"object"}"#.into(),
                effect: types::Effect::SideEffect,
            },
        ]
    }

    async fn call(name: String, input: String) -> types::Outcome {
        if name == "fixture_spin" {
            enco_sdk::host::log(enco_sdk::host::LogLevel::Info, "spin entered");
            loop {
                std::hint::spin_loop();
            }
        }
        if name == "fixture_echo" {
            types::Outcome::Ok(input)
        } else {
            types::Outcome::Failed(types::Failure {
                code: "tool.invalid_arguments".into(),
                message: format!("Unknown tool {name}"),
                retryable: false,
            })
        }
    }
}
