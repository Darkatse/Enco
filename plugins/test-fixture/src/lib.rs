//! Shared tool fixture for source registration, evaluation and cancellation.
use bindings::exports::enco::plugin::{lifecycle, tools};
use enco_sdk::types;
use schemars::JsonSchema;
use serde::Deserialize;

enco_sdk::export!(tools, Plugin);

struct Plugin;

/// Arguments of `fixture_echo`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Echo {
    /// Text to return unchanged.
    text: String,
}

/// `fixture_spin` takes no arguments.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Spin {}

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
                description: "Return the given text unchanged.".into(),
                input_schema: enco_sdk::input_schema::<Echo>().to_string(),
                effect: types::Effect::ReadOnly,
            },
            types::ToolSpec {
                name: "fixture_spin".into(),
                description: "Keep computing until the invocation is cancelled.".into(),
                input_schema: enco_sdk::input_schema::<Spin>().to_string(),
                effect: types::Effect::SideEffect,
            },
        ]
    }

    async fn call(name: String, input: String) -> types::Outcome {
        match name.as_str() {
            "fixture_echo" => match enco_sdk::parse_arguments::<Echo>(&input) {
                Ok(echo) => types::Outcome::Ok(serde_json::Value::String(echo.text).to_string()),
                Err(failure) => types::Outcome::Failed(failure),
            },
            "fixture_spin" => {
                enco_sdk::host::log(enco_sdk::host::LogLevel::Info, "spin entered");
                loop {
                    std::hint::spin_loop();
                }
            }
            _ => types::Outcome::Failed(types::Failure {
                code: "tool.invalid_arguments".into(),
                message: format!("Unknown tool {name}"),
                retryable: false,
            }),
        }
    }
}
