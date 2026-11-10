use crate::{CallContext, Tool, limits, snapshot::Handler};
use async_trait::async_trait;
use enco_core::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub(crate) const REPLY_SKIP: &str = "reply_skip";
pub(crate) const SESSION_SEND: &str = "session_send";
pub(crate) const SESSION_HALT: &str = "session_halt";
pub(crate) const SESSION_ARCHIVE: &str = "session_archive";
pub(crate) const SCHEDULE_CREATE: &str = "schedule_create";
pub(crate) const SCHEDULE_LIST: &str = "schedule_list";
pub(crate) const SCHEDULE_CANCEL: &str = "schedule_cancel";
/// The tools that act on child Sessions, absent where delegation is too deep.
pub(crate) const DELEGATION: [&str; 3] = [SESSION_SEND, SESSION_HALT, SESSION_ARCHIVE];

/// A call the Session actor answers from the Session's own facts, in call order.
///
/// `reply_skip` answers at once and may end the reply without sending it. The others only parse
/// their arguments when they execute; their answer and what they change come with the settlement.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorCall {
    /// Ends the reply without sending it.
    ReplySkip,
    /// Returns a child input to be committed by the Session actor.
    SessionSend,
    /// Reads or changes this Session's schedules.
    Schedule(ScheduleCall),
}

/// A scheduling call, answered from this Session's effective schedules when it settles.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduleCall {
    /// Creates a schedule with the settlement.
    Create,
    /// Lists the active schedules.
    List,
    /// Cancels one of the active schedules with the settlement.
    Cancel,
}

/// A tool the kernel declares, and what answers a call to it.
#[derive(Clone, Copy)]
enum Kind {
    /// Answered by the Session actor.
    Actor(ActorCall),
    /// Halts a child Session's subtree.
    SessionHalt,
    /// Halts and archives a child Session's subtree.
    SessionArchive,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateArguments {
    /// A future time for one reminder, in RFC 3339 with a UTC offset, such as
    /// `2026-10-24T09:00:00+08:00`.
    pub at: Option<String>,
    /// A rule for repeating reminders: minute hour day month weekday.
    pub cron: Option<String>,
    /// The IANA time zone a cron rule follows, such as `Asia/Shanghai`; the owner's by default.
    pub timezone: Option<String>,
    /// The text delivered to this Session when the reminder is due.
    pub message: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelArguments {
    /// The schedule to stop, as `schedule_list` shows it.
    pub id: ScheduleId,
}

impl Kind {
    /// Every kernel tool, in catalog order.
    const ALL: [Self; 7] = [
        Self::Actor(ActorCall::Schedule(ScheduleCall::Create)),
        Self::Actor(ActorCall::Schedule(ScheduleCall::List)),
        Self::Actor(ActorCall::Schedule(ScheduleCall::Cancel)),
        Self::Actor(ActorCall::ReplySkip),
        Self::Actor(ActorCall::SessionSend),
        Self::SessionHalt,
        Self::SessionArchive,
    ];

    fn spec(self) -> Result<ToolSpec, SchemaError> {
        match self {
            Kind::Actor(ActorCall::SessionSend) => ToolSpec::of::<SendArguments>(
                SESSION_SEND,
                "Send a task or follow-up message to a child Session. The call returns once the message is accepted; when its replies go to you, its answer arrives later as an incoming message. A new child starts with an empty history, so write the full goal, background, constraints, expected result and how to verify it; an existing child keeps its history.",
                Effect::ReadOnly,
            ),
            Kind::SessionHalt => ToolSpec::of::<ChildArguments>(
                SESSION_HALT,
                "Halt one of your child Sessions and its children, waiting until their work has stopped. Unanswered messages sent to them before the halt are left unanswered; send another message to continue.",
                Effect::Idempotent,
            ),
            Kind::SessionArchive => ToolSpec::of::<ChildArguments>(
                SESSION_ARCHIVE,
                "Halt one of your child Sessions and its children, then archive them. An archived Session accepts no more messages, and its name can be used for a new child Session.",
                Effect::Idempotent,
            ),
            Kind::Actor(ActorCall::Schedule(ScheduleCall::Create)) => ToolSpec::of::<CreateArguments>(
                SCHEDULE_CREATE,
                format!(
                    "Create a reminder delivered to this Session, even if Enco restarts. Give exactly \
                     one of at and cron; cron occurrences must be at least {} minutes apart. If Enco \
                     was not running, only the latest missed occurrence is delivered.",
                    limits::MIN_RECURRENCE_INTERVAL.as_secs() / 60
                ),
                Effect::ReadOnly,
            ),
            Kind::Actor(ActorCall::Schedule(ScheduleCall::List)) => Ok(ToolSpec {
                name: SCHEDULE_LIST.into(),
                description: "List this Session's active schedules, with their rules and next occurrences.".into(),
                input_schema: InputSchema::empty(),
                effect: Effect::ReadOnly,
            }),
            Kind::Actor(ActorCall::Schedule(ScheduleCall::Cancel)) => ToolSpec::of::<CancelArguments>(
                SCHEDULE_CANCEL,
                "Stop future occurrences of one of this Session's active schedules. Reminders \
                 already delivered are unaffected. Fails when the schedule is not this Session's \
                 or is no longer active.",
                Effect::ReadOnly,
            ),
            Kind::Actor(ActorCall::ReplySkip) => Ok(ToolSpec {
                name: REPLY_SKIP.into(),
                description: "End your turn without sending a reply. Use it only when nobody is waiting for a reply, for example after a reminder; otherwise the call fails. Call it by itself, after any other work. Text you write alongside it is not sent, but stays in your history for later.".into(),
                input_schema: InputSchema::empty(),
                effect: Effect::ReadOnly,
            }),
        }
    }
}

/// Native declarations available before their stateful handlers are constructed.
pub fn builtin_tool_specs() -> Result<Vec<ToolSpec>, SchemaError> {
    Kind::ALL.into_iter().map(Kind::spec).collect()
}

/// The code behind every kernel tool.
pub(crate) fn code() -> NativeCode {
    NativeCode {
        name: "kernel-builtin".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// What `reply_skip` answers when it ends the reply.
pub(crate) const REPLY_SKIPPED: &str =
    "Nothing was sent, including any text you wrote with this call.";

/// Every kernel tool in catalog order, each with what answers a call to it.
pub(crate) fn tools(
    subtrees: &Arc<crate::subtree::Subtrees>,
) -> Result<Vec<(ToolSpec, Handler)>, SchemaError> {
    Kind::ALL
        .into_iter()
        .map(|kind| {
            let spec = kind.spec()?;
            let handler = match kind {
                Kind::Actor(call) => Handler::Actor(call),
                Kind::SessionHalt | Kind::SessionArchive => Handler::Host(Arc::new(ChildTool {
                    subtrees: subtrees.clone(),
                    archive: matches!(kind, Kind::SessionArchive),
                    spec: spec.clone(),
                })),
            };
            Ok((spec, handler))
        })
        .collect()
}

/// `session_halt` and `session_archive`, which wait until a child's subtree has stopped.
struct ChildTool {
    subtrees: Arc<crate::subtree::Subtrees>,
    archive: bool,
    spec: ToolSpec,
}

#[async_trait]
impl Tool for ChildTool {
    fn code(&self) -> NativeCode {
        code()
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.execute(ctx.session, args).await {
            Ok(value) => Outcome::Ok {
                value,
                images: vec![],
                outgoing: None,
            },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl ChildTool {
    async fn execute(
        &self,
        session: SessionId,
        args: Map<String, Value>,
    ) -> Result<Value, Failure> {
        let ChildArguments { session: short } = parse_arguments(args)?;
        let halted = self
            .subtrees
            .child(session, &short, self.archive)
            .await
            .map_err(|error| child_failure(&short, error))?;
        let mut text = if self.archive {
            format!(
                "Archived `{short}` and its children; the name `{short}` is free for a new child Session."
            )
        } else {
            format!("Halted `{short}` and its children; send `{short}` a message to continue.")
        };
        for stopped in &halted.stopped {
            text.push_str(&format!(
                " `{}` had already stopped after a failure; it takes the halt once it runs again.",
                stopped.name
            ));
        }
        Ok(json!(text))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendArguments {
    /// The child Session's short name: one segment without slashes or whitespace.
    pub to: String,
    /// The task or follow-up message to send.
    pub text: String,
    /// The profile for a new child Session; omit when sending to an existing child.
    pub profile: Option<String>,
    /// The working directory for a new child Session, relative to your working directory or
    /// absolute; omit to use yours, and omit for an existing child.
    pub working_dir: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChildArguments {
    /// The child Session's short name, as `session_send` takes it.
    session: String,
}

fn child_failure(short: &str, error: crate::KernelError) -> Failure {
    use crate::{KernelError::Lookup, SessionLookupError};
    let (code, message) = match error {
        Lookup(SessionLookupError::InvalidName(error)) => {
            (code::TOOL_INVALID_ARGUMENTS, error.to_string())
        }
        Lookup(SessionLookupError::Unknown(_)) => (
            code::TOOL_FAILED,
            format!("you have no child Session named `{short}`"),
        ),
        error => (code::TOOL_FAILED, error.to_string()),
    };
    Failure {
        code: code.into(),
        message,
        retryable: false,
    }
}
