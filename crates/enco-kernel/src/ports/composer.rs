use enco_core::*;

/// Pure policy boundary: identical inputs must produce identical plans.
pub trait Composer: Send + Sync {
    /// Composer code recorded with every attempt.
    fn code(&self) -> CodeRef;
    /// Select, order and budget content without IO or hidden inputs.
    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError>;
}

/// Fixed Round inputs, with the selected budgets and recorded recovery facts for this composition.
#[derive(Clone)]
pub struct ComposeInput {
    /// UTC instant sampled once for this Round.
    pub now: DateTime<Utc>,
    /// Owner's time zone, shared with recurring schedules.
    pub timezone: Tz,
    /// Session metadata sampled for this Round.
    pub session: SessionRecord,
    /// Canonical history after the latest compaction.
    pub transcript: Transcript,
    /// Most recent reply plan; the composer decides whether to reuse it.
    pub previous_plan: Option<ContextPlan>,
    /// Candidate content from the configured sources, in priority order, plus omissions the
    /// kernel records itself, such as an unreadable previous plan.
    pub context: Contribution,
    /// Published plugins excluded from the Round's capabilities and their current reasons.
    pub unavailable: Vec<(String, Failure)>,
    /// All capabilities which may be disclosed this Round.
    pub tools: Vec<(CapabilityId, ToolSpec)>,
    /// Why this Round uses factory instructions and lifeline tools, if applicable.
    pub safe_mode: Option<SafeMode>,
    /// The reply model this composition is for.
    pub reply: ReplyTarget,
    /// The summarizing model's limits; absent when no compaction candidate is usable.
    pub compaction: Option<Budget>,
    /// Compactions this Round still allows; at zero the composer must return a plan or fail.
    pub compactions_left: u32,
    /// Local blob addresses supplied by the store for image annotations.
    pub image_paths: std::collections::HashMap<ContentHash, std::path::PathBuf>,
}

/// The reason for the Round's fixed recovery mode.
#[derive(Clone)]
pub enum SafeMode {
    /// The owner explicitly enabled the switch; only disabling it permits normal operation.
    Owner,
    /// This Run failed against the same registered behavior.
    Failed(Failure),
}

/// The reply model the kernel selected, and why earlier candidates were passed over.
#[derive(Clone)]
pub struct ReplyTarget {
    /// Limits of the selected model.
    pub budget: Budget,
    /// Non-text inputs accepted by the selected reply endpoint.
    pub accepts: std::collections::BTreeSet<Modality>,
    /// Candidates before it that are unavailable or exhausted in this Run, in configured order.
    pub skipped: Vec<EndpointFailure>,
}

/// A candidate and the reason the kernel passed it over.
#[derive(Clone)]
pub struct EndpointFailure {
    /// Configured name, for diagnostics rather than combination identity.
    pub endpoint: String,
    /// Current unavailability or the last recorded failure of this combination.
    pub failure: Failure,
}

/// Context limits supplied by configuration.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// Total model window including reserved output.
    pub context_tokens: std::num::NonZeroU32,
    /// Requested maximum generated tokens.
    pub max_output_tokens: std::num::NonZeroU32,
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
    /// Consumed Inbox Event, present only for inputs; the composer narrates its circumstances.
    pub event: Option<Event>,
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
    /// The request fits only after compaction, and no compaction candidate is usable.
    #[error("compaction is required but no compaction model is usable")]
    CompactionUnavailable,
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
