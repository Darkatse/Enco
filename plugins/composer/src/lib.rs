//! Plans model requests from recorded history, standing instructions and memories within the model's window.
mod compaction;
mod compose;
mod context;
mod history;
mod images;
mod limits;
mod recovery;

use bindings::exports::enco::plugin::{composer, lifecycle};
use enco_sdk::types::Failure;
use serde::Deserialize;

enco_sdk::export!(composer, Plugin);

struct Plugin;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {}

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "Builds model requests from recorded history, instructions and memories"
                .into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl composer::Guest for Plugin {
    fn options_schema(_config: String) -> String {
        enco_sdk::input_schema::<Options>().to_string()
    }

    fn compose(
        _options: String,
        input: crate::composer::ComposeInput,
    ) -> Result<crate::composer::Composition, Failure> {
        compose::compose(&input)
    }
}

fn failed(message: impl Into<String>) -> Failure {
    enco_sdk::protocol::failure("compose.failed", message, false)
}

// Budgeting uses a conservative estimate; the Provider reports actual usage.
fn tokens(bytes: usize) -> u32 {
    u32::try_from(bytes.div_ceil(3)).unwrap_or(u32::MAX)
}

fn estimate_tokens(text: &str) -> u32 {
    tokens(text.len())
}

/// Log positions order by epoch, then sequence.
fn key(pos: &crate::composer::LogPos) -> (u64, u64) {
    (pos.epoch, pos.seq)
}

fn inline(role: enco_sdk::types::Role, text: impl Into<String>) -> crate::composer::PlanItem {
    crate::composer::PlanItem::Message(crate::composer::Inline {
        role,
        text: text.into(),
        sources: vec![],
    })
}

#[cfg(test)]
mod tests;
