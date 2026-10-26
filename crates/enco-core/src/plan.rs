use crate::{CapabilityId, ContentHash, Effect, LogPos, Message, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The complete request plan, recorded by content address before sending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPlan {
    /// Messages in request order.
    pub items: Vec<PlanItem>,
    /// Disclosed capabilities together with their frozen definitions.
    pub tools: Vec<(CapabilityId, DisclosedTool)>,
    /// Requested output budget; absent means provider default.
    pub max_output_tokens: Option<u32>,
    /// Reasons content was not included in this request.
    pub omitted: Vec<Omission>,
}

impl ContextPlan {
    /// The plan's identity: the address of its complete JSON, whatever form stores it.
    pub fn hash(&self) -> Result<ContentHash, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| ContentHash::of(&bytes))
    }
}

/// A tool as a plan disclosed it to the model, kept verbatim.
///
/// A recorded schema is read back as it was sent and never checked again, so plans recorded under
/// earlier rules stay readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisclosedTool {
    /// Name the model proposes calls by.
    pub name: String,
    /// Instructions the model read.
    pub description: String,
    /// The argument schema as sent.
    pub input_schema: Value,
    /// Mutation semantics declared with the tool.
    pub effect: Effect,
}

impl From<&ToolSpec> for DisclosedTool {
    fn from(spec: &ToolSpec) -> Self {
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            input_schema: spec.input_schema.to_value(),
            effect: spec.effect,
        }
    }
}

/// Inline content or a reference to an immutable canonical message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanItem {
    /// Composer-created content recorded directly in this plan.
    Message {
        /// Inline content created by the composer, recorded verbatim in the plan.
        message: Message,
        /// The sources this message renders, in rendering order.
        sources: Vec<Source>,
    },
    /// Reference to a canonical Log message.
    Log {
        /// Entry whose canonical message is resolved from this Session’s Log.
        pos: LogPos,
        /// Whether this whole entry's images accompany its text in the request.
        images: bool,
    },
}

/// A source an inline message renders, named and hashed by the composer that wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// The composer's name for the source, such as `memory:<id>`.
    pub id: String,
    /// Content hash of the source's original text, before formatting.
    pub hash: ContentHash,
}

/// A visible explanation of content excluded from this request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Omission {
    /// Identifier of the omitted content, such as `memory:<id>`.
    pub source: String,
    /// Explanation visible during request inspection.
    pub reason: String,
}
