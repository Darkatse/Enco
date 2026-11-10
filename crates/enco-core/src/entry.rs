use crate::{
    AttemptId, CallId, CapabilityId, CodeRef, ContentHash, DateTime, Effect, Event, Failure, Image,
    LogPos, MappingOrigin, Message, Part, Role, RoundId, RunId, Settlement, ToolCall, ToolResult,
    Utc,
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
    /// A new Run was accepted.
    RunStarted {
        /// Run containing this fact.
        run: RunId,
    },
    /// A new execution boundary was accepted.
    RoundStarted {
        /// Run containing this fact.
        run: RunId,
        /// Round containing this fact.
        round: RoundId,
        /// Whether this Round uses only the lifeline.
        safe_mode: bool,
        /// Working directory pinned for this Round.
        working_dir: crate::WorkingDir,
    },
    /// Committed before calling the Provider; `plan` addresses the recorded ContextPlan.
    AttemptStarted {
        /// Round containing this fact.
        round: RoundId,
        /// Provider attempt associated with this fact.
        attempt: AttemptId,
        /// Purpose of this provider request.
        purpose: AttemptPurpose,
        /// Address of the frozen ContextPlan.
        plan: ContentHash,
        /// Composer code which constructed the frozen plan.
        composer: CodeRef,
        /// Provider code used by this Attempt.
        provider: CodeRef,
        /// Exact per-call parameters; credentials are never recorded.
        settings: crate::ProviderSettings,
    },
    /// A provider attempt completed or failed.
    AttemptSettled {
        /// Provider attempt associated with this fact.
        attempt: AttemptId,
        /// Durable settlement returned by the provider.
        result: AttemptResult,
        /// The recorded call that decoded the service's response.
        ///
        /// Absent when no response arrived, the request could not be built, the attempt was
        /// cancelled, or recovery wrote the settlement.
        origin: Option<MappingOrigin>,
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
        code: crate::ToolCode,
        /// Mutation semantics used when recovering an unsettled call.
        effect: Effect,
    },
    /// One settlement per call proposed by a completed reply, including calls never dispatched.
    ToolCallSettled {
        /// Host identity of the proposed tool call.
        call: CallId,
        /// Known or uncertain result of execution.
        outcome: Settlement,
        /// Exact model-visible result text.
        content: String,
        /// Address of the complete result when its inline text was truncated.
        full: Option<ContentHash>,
        /// Images stored by the settlement before this entry was committed.
        images: Vec<Image>,
        /// Content handed to the current reply destination with this settlement.
        outgoing: Option<crate::Outgoing>,
        /// Child delegation accepted atomically with this settlement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delegated: Option<Delegated>,
        /// A change to the Session's own schedules, committed with this settlement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<crate::ScheduleChange>,
    },
    /// All proposed calls have been settled and the Round has ended.
    RoundEnded {
        /// Round containing this fact.
        round: RoundId,
        /// Reason execution stopped.
        end: RoundEnd,
    },
    /// This Run will perform no more work.
    RunEnded {
        /// Run containing this fact.
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

/// Child Session delegation committed with a `session_send` settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delegated {
    /// The child the message went to.
    pub target: DelegationTarget,
    /// Event accepted into the child Session's Inbox.
    pub event: crate::EventId,
}

/// The child Session a delegation addressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DelegationTarget {
    /// A child that already existed.
    Existing(crate::SessionRef),
    /// A child this call created, by its creation record; the parent's Log is the authority over
    /// its children's names, and the child's row starts from this record.
    Created(crate::Creation),
}

impl DelegationTarget {
    /// The child's identity and full name.
    pub fn session(&self) -> crate::SessionRef {
        match self {
            Self::Existing(session) => session.clone(),
            Self::Created(creation) => crate::SessionRef {
                id: creation.id,
                name: creation.name.clone(),
            },
        }
    }
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
    /// The service turned the request away as larger than the model accepts.
    Overflow {
        /// What the service said about the request.
        overflow: Overflow,
    },
}

/// A service's refusal of a request larger than its model accepts, with the numbers it gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overflow {
    /// The request's input as the service counted it, when it says; above `accepted` when both
    /// are present.
    pub counted: Option<u64>,
    /// The most input the service accepts for this request, when it says, after any output it
    /// counts against the same window.
    pub accepted: Option<u64>,
    /// What the service said.
    pub message: String,
}

impl Overflow {
    /// This refusal as a failure, for a caller that cannot send a smaller request.
    ///
    /// It ends a Round that has shrunk its request as often as it may, and reports a configuration
    /// probe the service turned away.
    pub fn failure(&self) -> Failure {
        Failure {
            code: crate::code::CONTEXT_OVERFLOW.into(),
            message: format!(
                "the service turned the request away as too large: {}",
                self.message
            ),
            retryable: false,
        }
    }
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
    /// The reply ended with `reply_skip` and stays in the Log as a note.
    ReplySkipped,
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
        /// The active configuration and plugin code when this Round failed.
        active: crate::ActiveCode,
    },
}

/// Why this Run stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEnd {
    /// The operation completed normally.
    Completed,
    /// The Run exhausted its Round allowance.
    BudgetExhausted,
    /// The previous process ended before settlement.
    Interrupted,
    /// Cancellation was requested and work has stopped.
    Cancelled,
    /// The owner went back in the conversation, which stopped it.
    Backtracked,
    /// A halt of this Session or an ancestor stopped it.
    Halted,
    /// The operation stopped with a known failure.
    Failed {
        /// Classification and details of the failure.
        failure: Failure,
    },
}

impl EntryBody {
    /// The child Session this fact creates: the creation record of a delegation that made it.
    ///
    /// The child's row starts from this record, both when the parent commits it and when the
    /// rows are rebuilt from the facts.
    pub fn created_child(&self) -> Option<&crate::Creation> {
        match self {
            Self::ToolCallSettled {
                delegated:
                    Some(Delegated {
                        target: DelegationTarget::Created(creation),
                        ..
                    }),
                ..
            } => Some(creation),
            _ => None,
        }
    }

    /// Resolve a durable entry's model-facing form using its recorded associations.
    /// This mapping is part of the persistent format, not composer policy.
    pub fn canonical_message(
        &self,
        purpose: Option<AttemptPurpose>,
        call: Option<&ToolCall>,
    ) -> Option<Message> {
        match self {
            Self::EventConsumed { event } if !event.body.leaves_inputs() => {
                Some(event.canonical_message())
            }
            // An unusable reply stays in the Log but is not part of the conversation.
            Self::AttemptSettled {
                result: result @ AttemptResult::Completed { message, .. },
                ..
            } if purpose == Some(AttemptPurpose::Reply)
                && result.failure(AttemptPurpose::Reply).is_none() =>
            {
                Some(message.clone())
            }
            Self::ToolCallSettled {
                call: id,
                outcome,
                content,
                images,
                ..
            } => call.map(|call| Message {
                role: Role::Tool,
                parts: std::iter::once(Part::ToolResult(ToolResult {
                    call: *id,
                    provider_id: call.provider_id.clone(),
                    content: content.clone(),
                    is_error: !matches!(outcome, Settlement::Ok),
                }))
                .chain(images.iter().cloned().map(Part::Image))
                .collect(),
            }),
            _ => None,
        }
    }
}

impl AttemptResult {
    /// Why this result uses up its combination's allowance for `purpose`.
    ///
    /// That is its failure, or why a completion cannot be used, which stays recorded as the
    /// service answered it. Absent for a usable completion, and for an overflow: a request turned away as too large
    /// asks for a smaller request on the same combination, not another candidate.
    pub fn failure(&self, purpose: AttemptPurpose) -> Option<Failure> {
        match self {
            Self::Failed { failure } => Some(failure.clone()),
            Self::Overflow { .. } => None,
            Self::Completed { message, stop, .. } => {
                let empty = message.joined_text().trim().is_empty();
                let problem = match purpose {
                    // A cut-off summary would replace history it never finished covering.
                    AttemptPurpose::Compaction if *stop == StopReason::MaxTokens => {
                        "a summary cut off at max_output_tokens"
                    }
                    AttemptPurpose::Compaction if empty => "an empty summary",
                    AttemptPurpose::Reply if empty && message.tool_calls().next().is_none() => {
                        "no text or tool calls"
                    }
                    AttemptPurpose::Reply | AttemptPurpose::Compaction => return None,
                };
                Some(Failure {
                    code: crate::code::PROVIDER_BAD_RESPONSE.into(),
                    message: format!(
                        "provider returned {problem}; stop: {}",
                        serde_json::json!(stop)
                    ),
                    retryable: false,
                })
            }
        }
    }
}
