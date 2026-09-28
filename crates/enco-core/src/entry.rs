use crate::{
    AttemptId, CallId, CapabilityId, CodeRef, ContentHash, DateTime, Effect, Event, Failure,
    LogPos, Message, Outcome, Part, Role, RoundId, RunId, ToolCall, ToolResult, Utc,
};
use serde::{Deserialize, Serialize};

/// One immutable fact at a structurally ordered position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Position assigned by the Session owner.
    pub pos: LogPos,
    /// Observation time for display, never for ordering.
    pub at: DateTime<Utc>,
    /// Accepted input or execution transition at this Log position.
    pub body: EntryBody,
}

/// Facts accepted by the Session owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryBody {
    /// Accept an Inbox Event into Session history and consume its row in the same Commit.
    EventConsumed {
        /// The complete Event accepted from this Session’s Inbox.
        event: Event,
    },
    /// A new activation was accepted.
    RunStarted {
        /// Activation containing this fact.
        run: RunId,
    },
    /// A new execution boundary was accepted.
    RoundStarted {
        /// Activation containing this fact.
        run: RunId,
        /// Round containing this fact.
        round: RoundId,
        /// Whether this Round uses only the lifeline.
        safe_mode: bool,
    },
    /// Committed before calling the Provider; `plan` addresses the serialized ContextPlan blob.
    AttemptStarted {
        /// Round containing this fact.
        round: RoundId,
        /// Provider attempt associated with this fact.
        attempt: AttemptId,
        /// Purpose of this provider request.
        purpose: AttemptPurpose,
        /// Address of the frozen ContextPlan.
        plan: ContentHash,
        /// Code which constructed the frozen plan.
        composer: CodeRef,
        /// Adapter code used by this Attempt.
        provider: CodeRef,
    },
    /// A provider attempt completed or failed.
    AttemptSettled {
        /// Provider attempt associated with this fact.
        attempt: AttemptId,
        /// Durable settlement returned by the provider.
        result: AttemptResult,
    },
    /// Committed before execution. Calls rejected by dispatch do not receive this entry.
    ToolCallStarted {
        /// Round containing this fact.
        round: RoundId,
        /// Host identity of the proposed tool call.
        call: CallId,
        /// Node-qualified capability bound to the call.
        capability: CapabilityId,
        /// Implementation bound to this invocation.
        code: CodeRef,
        /// Mutation semantics used when recovering an unsettled call.
        effect: Effect,
    },
    /// One settlement per call proposed by a completed reply, including calls never dispatched.
    ToolCallSettled {
        /// Host identity of the proposed tool call.
        call: CallId,
        /// Known or uncertain result of execution.
        outcome: Outcome,
        /// Exact model-visible result text.
        content: String,
        /// Address of the complete result when its inline text was truncated.
        full: Option<ContentHash>,
    },
    /// All proposed calls have been settled and the Round has ended.
    RoundEnded {
        /// Round containing this fact.
        round: RoundId,
        /// Reason execution stopped.
        end: RoundEnd,
    },
    /// This activation will perform no more work.
    RunEnded {
        /// Activation containing this fact.
        run: RunId,
        /// Reason execution stopped.
        end: RunEnd,
    },
    /// Replace the Transcript prefix through `upto` with `summary`; retain the original Log entries.
    Compacted {
        /// Inclusive Log position hidden by the summary.
        upto: LogPos,
        /// Model-generated replacement for the summarized history.
        summary: String,
        /// Provider attempt associated with this fact.
        attempt: AttemptId,
    },
}

/// Whether the model is replying or summarizing prior rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptPurpose {
    /// A reply whose tool calls may be dispatched.
    Reply,
    /// A summary request whose output replaces earlier context.
    Compaction,
}

/// The durable settlement of a provider attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttemptResult {
    /// The operation completed normally.
    Completed {
        /// Accepted Assistant output, including tool proposals and adapter extensions.
        message: Message,
        /// Provider-reported token usage.
        usage: Usage,
        /// Provider-reported stop condition.
        stop: StopReason,
    },
    /// The operation stopped with a known failure.
    Failed {
        /// Classification and details of the failure.
        failure: Failure,
    },
}

/// Token counts reported by the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Total input tokens reported by the provider.
    pub input_tokens: u64,
    /// Total output tokens reported by the provider.
    pub output_tokens: u64,
    /// Cached input tokens, if the provider reports them.
    pub cached_input_tokens: Option<u64>,
}

/// Provider-reported completion boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The provider finished its reply.
    EndTurn,
    /// The provider requested tools.
    ToolCalls,
    /// The output limit was reached.
    MaxTokens,
    /// A provider-specific stopping condition.
    Other,
}

/// Why this Round stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoundEnd {
    /// A reply without tool calls was recorded.
    Replied,
    /// All proposed tool calls have durable outcomes.
    ToolsSettled,
    /// The previous process ended before settlement.
    Interrupted,
    /// Cancellation was requested and work has stopped.
    Cancelled,
    /// The operation stopped with a known failure.
    Failed {
        /// Classification and details of the failure.
        failure: Failure,
    },
}

/// Why this activation stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEnd {
    /// The operation completed normally.
    Completed,
    /// The activation exhausted its Round allowance.
    BudgetExhausted,
    /// The previous process ended before settlement.
    Interrupted,
    /// Cancellation was requested and work has stopped.
    Cancelled,
    /// The operation stopped with a known failure.
    Failed {
        /// Classification and details of the failure.
        failure: Failure,
    },
}

impl EntryBody {
    /// Resolve a durable entry's model-facing form using its recorded associations.
    /// This mapping is part of the persistent format, not composer policy.
    pub fn canonical_message(
        &self,
        purpose: Option<AttemptPurpose>,
        call: Option<&ToolCall>,
    ) -> Option<Message> {
        match self {
            Self::EventConsumed { event } => Some(event.canonical_message()),
            Self::AttemptSettled {
                result: AttemptResult::Completed { message, .. },
                ..
            } if purpose == Some(AttemptPurpose::Reply) => Some(message.clone()),
            Self::ToolCallSettled {
                call: id,
                outcome,
                content,
                ..
            } => call.map(|call| Message {
                role: Role::Tool,
                parts: vec![Part::ToolResult(ToolResult {
                    call: *id,
                    provider_id: call.provider_id.clone(),
                    content: content.clone(),
                    is_error: !matches!(outcome, Outcome::Ok { .. }),
                })],
            }),
            _ => None,
        }
    }
}

impl RoundEnd {
    /// Map a terminal Round outcome to its Run outcome.
    /// Replies and settled tools require the Run loop to inspect the Inbox or continue.
    pub fn terminal_run_end(&self) -> Option<RunEnd> {
        match self {
            Self::Interrupted => Some(RunEnd::Interrupted),
            Self::Cancelled => Some(RunEnd::Cancelled),
            Self::Failed { failure } => Some(RunEnd::Failed {
                failure: failure.clone(),
            }),
            Self::Replied | Self::ToolsSettled => None,
        }
    }
}
