use async_trait::async_trait;
use enco_core::*;
use std::{borrow::Borrow, path::PathBuf};

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
    /// Canonical history after the latest compaction.
    pub transcript: Transcript<LocatedImage>,
    /// Most recent reply plan; the composer decides whether to reuse it.
    pub previous_plan: Option<ContextPlan>,
    /// The text of the owner's standing instructions; absent when there are none.
    pub instructions: Option<String>,
    /// Pinned memories first, then those recalled for the latest input, most relevant first.
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
    /// The summarizing model's limits; absent when no compaction candidate is usable.
    pub compaction: Option<Budget>,
    /// Compactions this Round still allows; at zero the composer must return a plan or fail.
    pub compactions_left: u32,
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
#[derive(Debug, Clone)]
pub struct Transcript<I = Image> {
    /// Latest replacement for summarized history.
    pub summary: Option<String>,
    /// Canonical messages still visible after that summary, in ascending Log position order.
    pub items: Vec<TranscriptItem<I>>,
    /// Complete Round boundaries eligible for compaction, in ascending Log position order.
    pub round_ends: Vec<LogPos>,
    /// Run outcomes after the summary, in Log order; the composer decides how to describe them.
    pub run_ends: Vec<(LogPos, RunEnd)>,
}

/// One canonical message and its immutable source.
#[derive(Debug, Clone)]
pub struct TranscriptItem<I = Image> {
    /// Position of the originating fact.
    pub pos: LogPos,
    /// Canonical message used when resolving a plan reference.
    pub message: Message<I>,
    /// Provider generation of the producing Attempt, present only for Assistant messages.
    pub generation: Option<GenerationId>,
    /// Consumed Inbox Event, present only for inputs; the composer narrates its circumstances.
    pub event: Option<Event>,
}

/// An image reference with the node-local address supplied by its store.
#[derive(Debug, Clone)]
pub struct LocatedImage {
    /// Immutable image identity and metadata.
    pub image: Image,
    /// Local file holding the bytes.
    pub path: PathBuf,
}

impl Borrow<Image> for LocatedImage {
    fn borrow(&self) -> &Image {
        &self.image
    }
}

impl<I> Default for Transcript<I> {
    fn default() -> Self {
        Self {
            summary: None,
            items: Vec::new(),
            round_ends: Vec::new(),
            run_ends: Vec::new(),
        }
    }
}

impl<I> Transcript<I> {
    /// Change the image representation without copying the other message content.
    pub fn map_images<J>(self, mut image: impl FnMut(I) -> J) -> Transcript<J> {
        Transcript {
            summary: self.summary,
            round_ends: self.round_ends,
            run_ends: self.run_ends,
            items: self
                .items
                .into_iter()
                .map(|item| TranscriptItem {
                    pos: item.pos,
                    generation: item.generation,
                    event: item.event,
                    message: Message {
                        role: item.message.role,
                        parts: item
                            .message
                            .parts
                            .into_iter()
                            .map(|part| match part.retype() {
                                Ok(part) => part,
                                Err(value) => Part::Image(image(value)),
                            })
                            .collect(),
                    },
                })
                .collect(),
        }
    }
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
    /// Requested output budget; absent means provider default.
    pub max_output_tokens: Option<u32>,
    /// Content this policy left out, with its reasons.
    pub omitted: Vec<Omission>,
}

/// A ready reply request or a request to summarize earlier complete Rounds first.
pub enum Composition {
    /// Ready for a reply attempt.
    Plan(CompositionPlan),
    /// Summarize an inclusive prefix ending at a complete Round.
    Compact {
        /// Inclusive compaction boundary.
        upto: LogPos,
        /// Tool-free request to generate the summary.
        plan: CompositionPlan,
    },
}
