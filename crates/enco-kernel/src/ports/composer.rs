use enco_core::*;

/// Pure policy boundary: identical inputs must produce identical plans.
pub trait Composer: Send + Sync {
    /// Composer code recorded with every attempt.
    fn code(&self) -> CodeRef;
    /// Select, order and budget content without IO or hidden inputs.
    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError>;
}

/// Current state sampled once for this Round; only Transcript changes after compaction.
#[derive(Clone)]
pub struct ComposeInput {
    /// Owner-local time supplied by the kernel.
    pub now: DateTime<FixedOffset>,
    /// Session metadata sampled for this Round.
    pub session: SessionRecord,
    /// Canonical history after the latest compaction.
    pub transcript: Transcript,
    /// Most recent reply plan; the composer decides whether to reuse it.
    pub previous_plan: Option<ContextPlan>,
    /// Candidate content from the configured sources, in priority order, plus omissions the
    /// kernel records itself, such as an unreadable previous plan.
    pub context: Contribution,
    /// All capabilities which may be disclosed this Round.
    pub tools: Vec<(CapabilityId, ToolSpec)>,
    /// Whether only factory instructions and lifeline tools may be used.
    pub safe_mode: bool,
    /// Endpoints, budgets and requirements selected for this Round.
    pub profile: crate::Profile,
    /// Compactions this Round still allows; at zero the composer must return a plan or fail.
    pub compactions_left: u32,
}

/// Context limits supplied by configuration.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// Total model window including reserved output.
    pub context_tokens: u32,
    /// Requested maximum generated tokens.
    pub max_output_tokens: u32,
}

/// Model-facing projection of immutable Log facts.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    /// Latest replacement for summarized history.
    pub summary: Option<String>,
    /// Canonical messages still visible after that summary, in ascending Log position order.
    pub items: Vec<TranscriptItem>,
    /// Complete Round boundaries eligible for compaction, in ascending Log position order.
    pub round_ends: Vec<LogPos>,
    /// Run outcomes after the summary, in Log order; the composer decides how to describe them.
    pub run_ends: Vec<(LogPos, RunEnd)>,
}

/// One canonical message and its immutable source.
#[derive(Debug, Clone)]
pub struct TranscriptItem {
    /// Position of the originating fact.
    pub pos: LogPos,
    /// Canonical message used when resolving a plan reference.
    pub message: Message,
    /// Provider generation of the producing Attempt, present only for Assistant messages.
    pub generation: Option<GenerationId>,
    /// Recorded arrival time, present only for Inbox inputs; the composer decides how to show it.
    pub received_at: Option<DateTime<Utc>>,
}

/// A ready reply request or a request to summarize earlier complete Rounds first.
pub enum Composition {
    /// Ready for a reply attempt.
    Plan(ContextPlan),
    /// Summarize an inclusive prefix ending at a complete Round.
    Compact {
        /// Inclusive compaction boundary.
        upto: LogPos,
        /// Tool-free request to generate the summary.
        plan: ContextPlan,
    },
}

/// A policy could not produce a valid request.
#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    /// Even the smallest valid request exceeds the model's window.
    #[error("context overflow: need {needed} tokens, window is {window}")]
    ContextOverflow {
        /// Estimated total size.
        needed: u32,
        /// Configured context window.
        window: u32,
    },
    /// Policy inputs could not be composed.
    #[error("{0}")]
    Invalid(String),
}
