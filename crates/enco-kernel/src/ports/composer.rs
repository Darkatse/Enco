use async_trait::async_trait;
use enco_core::*;
use std::{collections::HashMap, path::PathBuf};

/// Pure policy boundary: identical inputs must produce identical plans.
#[async_trait]
pub trait Composer: Send + Sync {
    /// Select, order and budget content using options already checked against this generation.
    async fn compose(
        &self,
        options: &crate::ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure>;
}

/// Fixed Round inputs, with the selected budgets and recorded recovery facts for this composition.
#[derive(Clone)]
pub struct ComposeInput {
    /// Identity used to seed this Round's deterministic runtime.
    pub round: RoundId,
    /// Absolute home location used to describe paths to the Agent.
    pub home: PathBuf,
    /// UTC instant sampled once for this Round.
    pub now: DateTime<Utc>,
    /// Owner's time zone, shared with recurring schedules.
    pub timezone: Tz,
    /// Session metadata sampled for this Round.
    pub session: SessionRecord,
    /// Unarchived direct children in creation order.
    pub children: Vec<Child>,
    /// Profiles available to new children; empty when delegation is unavailable.
    pub profiles: Vec<DelegableProfile>,
    /// Canonical history after the latest compaction.
    pub transcript: Transcript,
    /// Where the blobs the transcript refers to are stored, by content hash: its images, and the
    /// attachments and files of its inputs.
    pub blob_paths: HashMap<ContentHash, PathBuf>,
    /// Plan of the latest completed reply request; the composer decides whether to reuse it.
    pub previous_plan: Option<ContextPlan>,
    /// Whether the owner asked to compact the conversation before this Round's reply.
    pub compaction_requested: bool,
    /// Whether this Session and all its ancestors permit owner instructions and memories.
    pub owner_context: bool,
    /// The owner instructions; absent when missing, in safe mode or without the owner's context.
    pub owner_instructions: Option<String>,
    /// The selected profile's instructions; absent when missing or in safe mode.
    pub profile_instructions: Option<String>,
    /// Pinned memories first, then those recalled for the latest input, most relevant first;
    /// empty in safe mode or without the owner's context.
    pub memories: Vec<Memory>,
    /// Content the host could not supply, recorded with the plan whatever the composer returns.
    pub omitted: Vec<Omission>,
    /// Published plugins excluded from the Round's capabilities and their current reasons.
    pub unavailable: Vec<(String, Failure)>,
    /// All capabilities which may be disclosed this Round.
    pub tools: Vec<(CapabilityId, ToolSpec)>,
    /// Capabilities that every reply plan of this Session must disclose.
    pub lifeline: Vec<CapabilityId>,
    /// Why this Round uses factory instructions and lifeline tools, if applicable.
    pub safe_mode: Option<SafeMode>,
    /// The reply model this composition is for.
    pub reply: ReplyTarget,
    /// The summarizing model; absent when no compaction candidate is usable.
    pub compaction: Option<CompactionTarget>,
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
    /// Token usage reported for `previous_plan`, present only when this target's combination
    /// served it: another model counts tokens differently.
    pub previous_usage: Option<Usage>,
    /// The latest plan this target's combination turned away as too large in this Round that the
    /// transcript still holds every Log item of, a reply or a summary.
    pub overflowed: Option<Overflowed>,
}

/// The summarizing model the kernel selected.
#[derive(Clone)]
pub struct CompactionTarget {
    /// Limits of the selected model.
    pub budget: Budget,
    /// As on [`ReplyTarget::overflowed`], for the combination behind this target.
    pub overflowed: Option<Overflowed>,
}

/// A plan its target turned away as too large, and what the service said about it.
#[derive(Clone)]
pub struct Overflowed {
    /// The plan as recorded with the turned-away Attempt.
    pub plan: ContextPlan,
    /// The service's refusal.
    pub overflow: Overflow,
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

/// Text written by a composer, or a reference to a recorded message.
#[derive(Debug, Clone)]
pub enum CompositionItem {
    /// Inline text and the sources it renders.
    Message {
        /// Speaker of the text.
        role: Role,
        /// Text written by this composition.
        text: String,
        /// Sources used to write it, in rendering order.
        sources: Vec<Source>,
    },
    /// An immutable message from the transcript.
    Log {
        /// Position of the message.
        pos: LogPos,
        /// Whether its images accompany the text.
        images: bool,
    },
}

/// Composer output before the kernel freezes the Round's tool definitions with it.
#[derive(Debug, Clone)]
pub struct CompositionPlan {
    /// Messages in request order.
    pub items: Vec<CompositionItem>,
    /// Capabilities selected from this Round.
    pub tools: Vec<CapabilityId>,
    /// The most tokens the reply may use; the kernel checks it against the target's budget.
    pub max_output_tokens: u32,
    /// Content this policy left out, with its reasons.
    pub omitted: Vec<Omission>,
}

/// A ready reply request or a request to summarize earlier complete Rounds first.
pub enum Composition {
    /// Ready for a reply attempt.
    Plan(CompositionPlan),
    /// Summarize an inclusive prefix ending at a complete Round.
    Compact {
        /// Inclusive compaction boundary: a Round end before the latest transcript item, so the
        /// reply after the summary has history to answer.
        upto: LogPos,
        /// Tool-free request to generate the summary.
        plan: CompositionPlan,
    },
}

/// A child of this Session, by the facts that change only at moments its parent sees.
#[derive(Debug, Clone)]
pub struct Child {
    /// Its name relative to this Session.
    pub name: String,
    /// Its current profile.
    pub profile: ProfileName,
    /// Whether this Session waits for its answer, how it was halted and where its replies go.
    pub state: ChildState,
}

/// A reply endpoint's declared model capabilities and budget.
#[derive(Clone)]
pub struct ProfileEndpoint {
    /// Configured endpoint name.
    pub endpoint: String,
    /// Configured model name.
    pub model: String,
    /// Non-text inputs this model accepts.
    pub accepts: std::collections::BTreeSet<Modality>,
    /// Context and output limits.
    pub budget: Budget,
}

/// An available profile for a newly delegated child.
#[derive(Clone)]
pub struct DelegableProfile {
    /// Validated profile name.
    pub name: ProfileName,
    /// Owner-written purpose.
    pub description: String,
    /// Reply endpoints in configured order.
    pub reply: Vec<ProfileEndpoint>,
}
