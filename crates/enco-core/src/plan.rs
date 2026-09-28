use crate::{CapabilityId, LogPos, Message, ToolSpec};
use serde::{Deserialize, Serialize};

/// The composer’s complete request plan, recorded as a content-addressed blob before sending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPlan {
    /// Messages in request order.
    pub items: Vec<PlanItem>,
    /// Disclosed capabilities together with their frozen definitions.
    pub tools: Vec<(CapabilityId, ToolSpec)>,
    /// Requested output budget; absent means provider default.
    pub max_output_tokens: Option<u32>,
    /// Reasons content was not included in this request.
    pub omitted: Vec<Omission>,
}

/// Inline content or a reference to an immutable canonical message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanItem {
    /// Composer-created content recorded directly in this plan.
    Message {
        /// Inline content created by the composer, recorded verbatim in the plan.
        message: Message,
    },
    /// Reference to a canonical Log message.
    Log {
        /// Entry whose canonical message is resolved from this Session’s Log.
        pos: LogPos,
    },
}

/// A visible explanation of content excluded from this request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Omission {
    /// Source or candidate identifier for the omitted content.
    pub source: String,
    /// Explanation visible during request inspection.
    pub reason: String,
}

/// Candidate content and omissions supplied by one ContextSource for this Round.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    /// Candidate content in descending priority.
    pub candidates: Vec<Candidate>,
    /// Content this source could not supply, with reasons for the composer to record.
    pub omitted: Vec<Omission>,
}

/// Content offered to the composer; source-prefixed IDs make omissions traceable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Source-qualified ID, such as `workspace:AGENTS.md` or `memory:<MemoryId>`.
    pub id: String,
    /// Semantic category of this content.
    pub kind: CandidateKind,
    /// Current source content that the composer may include within its budget.
    pub text: String,
}

/// Semantic category interpreted by the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    /// Standing instructions supplied by a context source.
    Instruction,
    /// A candidate durable memory.
    Memory,
}
