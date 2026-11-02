use crate::{CallContext, ScheduleError, Scheduled, Schedules, Tool, limits};
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
/// The tools that act on child Sessions, absent where delegation is too deep.
pub(crate) const DELEGATION: [&str; 3] = [SESSION_SEND, SESSION_HALT, SESSION_ARCHIVE];

/// A built-in call whose conditions the Session actor checks against its own Log; the scheduling
/// tools only call the Scheduler and are ordinary tools.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinCall {
    /// Ends the reply without sending it.
    ReplySkip,
    /// Returns a child input to be committed by the Session actor.
    SessionSend,
}

impl BuiltinCall {
    /// The built-in call a host tool of this name is; plugin tools cannot take host names.
    pub(crate) fn named(name: &str) -> Option<Self> {
        match name {
            REPLY_SKIP => Some(Self::ReplySkip),
            SESSION_SEND => Some(Self::SessionSend),
            _ => None,
        }
    }
}

enum Kind {
    SessionSend,
    SessionHalt,
    SessionArchive,
    Create,
    List,
    Cancel,
    ReplySkip,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateArguments {
    /// A future time for one reminder, in RFC 3339 with a UTC offset, such as
    /// `2026-10-24T09:00:00+08:00`.
    at: Option<String>,
    /// A rule for repeating reminders: minute hour day month weekday.
    cron: Option<String>,
    /// The IANA time zone a cron rule follows, such as `Asia/Shanghai`; the owner's by default.
    timezone: Option<String>,
    /// The text delivered to this Session when the reminder is due.
    message: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CancelArguments {
    /// The schedule to stop, as `schedule_list` shows it.
    id: ScheduleId,
}

impl Kind {
    const ALL: [Self; 7] = [
        Self::Create,
        Self::List,
        Self::Cancel,
        Self::ReplySkip,
        Self::SessionSend,
        Self::SessionHalt,
        Self::SessionArchive,
    ];
    fn spec(&self) -> Result<ToolSpec, SchemaError> {
        match self {
            Kind::SessionSend => ToolSpec::of::<SendArguments>(
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
            Kind::Create => ToolSpec::of::<CreateArguments>(
                "schedule_create",
                format!(
                    "Create a reminder delivered to this Session, even if Enco restarts. Give exactly \
                     one of at and cron; cron occurrences must be at least {} minutes apart. If Enco \
                     was not running, only the latest missed occurrence is delivered.",
                    limits::MIN_RECURRENCE_INTERVAL.as_secs() / 60
                ),
                Effect::SideEffect,
            ),
            Kind::List => Ok(ToolSpec {
                name: "schedule_list".into(),
                description: "List active schedules addressed to this Session, including their rules and next occurrences.".into(),
                input_schema: InputSchema::empty(),
                effect: Effect::ReadOnly,
            }),
            Kind::Cancel => ToolSpec::of::<CancelArguments>(
                "schedule_cancel",
                "Stop future occurrences of a schedule. Already delivered reminders are unaffected. \
                 Returns false when it is done, cancelled, or absent.",
                Effect::SideEffect,
            ),
            Kind::ReplySkip => Ok(ToolSpec {
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
    Kind::ALL.iter().map(Kind::spec).collect()
}

struct BuiltinTool {
    schedules: Schedules,
    subtrees: Arc<crate::subtree::Subtrees>,
    kind: Kind,
    spec: ToolSpec,
}

pub(crate) fn tools(
    schedules: Schedules,
    subtrees: Arc<crate::subtree::Subtrees>,
) -> Result<Vec<Arc<dyn Tool>>, SchemaError> {
    Kind::ALL
        .into_iter()
        .map(|kind| {
            Ok(Arc::new(BuiltinTool {
                schedules: schedules.clone(),
                subtrees: subtrees.clone(),
                spec: kind.spec()?,
                kind,
            }) as Arc<dyn Tool>)
        })
        .collect()
}

#[async_trait]
impl Tool for BuiltinTool {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "kernel-builtin".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
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

impl BuiltinTool {
    async fn execute(
        &self,
        session: SessionId,
        args: Map<String, Value>,
    ) -> Result<Value, Failure> {
        match self.kind {
            #[expect(
                clippy::unreachable,
                reason = "dispatch executes SessionSend as a typed actor-owned call instead of invoking this handler"
            )]
            Kind::SessionSend => {
                unreachable!("the actor executes session_send and commits its returned input")
            }
            Kind::SessionHalt | Kind::SessionArchive => {
                let ChildArguments { session: short } = parse_arguments(args)?;
                let archive = matches!(self.kind, Kind::SessionArchive);
                let halted = self
                    .subtrees
                    .child(session, &short, archive)
                    .await
                    .map_err(|error| child_failure(&short, error))?;
                let mut text = if archive {
                    format!(
                        "Archived `{short}` and its children; the name `{short}` is free for a new child Session."
                    )
                } else {
                    format!(
                        "Halted `{short}` and its children; send `{short}` a message to continue."
                    )
                };
                for stopped in &halted.stopped {
                    text.push_str(&format!(
                        " `{}` had already stopped after a failure; it takes the halt once it runs again.",
                        stopped.name
                    ));
                }
                Ok(json!(text))
            }
            Kind::Create => {
                let arguments: CreateArguments = parse_arguments(args)?;
                let scheduled = self
                    .schedules
                    .create(session, arguments.rule()?, arguments.message)
                    .await
                    .map_err(error)?;
                Ok(scheduled.into_json())
            }
            Kind::List => {
                let schedules = self.schedules.list(Some(session)).await.map_err(error)?;
                Ok(json!(
                    schedules
                        .into_iter()
                        .map(Scheduled::into_json)
                        .collect::<Vec<_>>()
                ))
            }
            Kind::Cancel => {
                let CancelArguments { id } = parse_arguments(args)?;
                let cancelled = self.schedules.cancel(id).await.map_err(error)?;
                Ok(json!({ "cancelled": cancelled }))
            }
            Kind::ReplySkip => Ok(json!(
                "Nothing was sent, including any text you wrote with this call."
            )),
        }
    }
}

impl CreateArguments {
    fn rule(&self) -> Result<ScheduleRule, Failure> {
        match (&self.at, &self.cron) {
            (Some(at), None) => {
                if self.timezone.is_some() {
                    return Err(invalid("timezone only applies to cron"));
                }
                let at = DateTime::parse_from_rfc3339(at)
                    .map_err(|e| invalid(format!("at must include a UTC offset: {e}")))?
                    .to_utc();
                Ok(ScheduleRule::Once { at })
            }
            (None, Some(cron)) => Ok(ScheduleRule::Cron {
                expr: cron.clone(),
                timezone: self
                    .timezone
                    .as_deref()
                    .map(|zone| {
                        zone.parse::<Tz>()
                            .map_err(|e| invalid(format!("timezone must be an IANA name: {e}")))
                    })
                    .transpose()?,
            }),
            _ => Err(invalid("give exactly one of at or cron")),
        }
    }
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure {
        code: code::TOOL_INVALID_ARGUMENTS.into(),
        message: message.into(),
        retryable: false,
    }
}

fn error(error: ScheduleError) -> Failure {
    let code = match error {
        ScheduleError::EmptyMessage
        | ScheduleError::UnknownSession(_)
        | ScheduleError::InvalidCron(_)
        | ScheduleError::NoOccurrenceAfter(_)
        | ScheduleError::TooFrequent => code::TOOL_INVALID_ARGUMENTS,
        _ => code::TOOL_FAILED,
    };
    Failure {
        code: code.into(),
        message: error.to_string(),
        retryable: false,
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
