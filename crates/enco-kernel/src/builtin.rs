use crate::{CallContext, ScheduleError, Scheduled, Schedules, Tool, limits};
use async_trait::async_trait;
use enco_core::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;
enum Kind {
    Create,
    List,
    Cancel,
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
    const ALL: [Self; 3] = [Self::Create, Self::List, Self::Cancel];
    fn spec(&self) -> Result<ToolSpec, SchemaError> {
        match self {
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
        }
    }
}

/// Native declarations available before their stateful handlers are constructed.
pub fn builtin_tool_specs() -> Result<Vec<ToolSpec>, SchemaError> {
    Kind::ALL.iter().map(Kind::spec).collect()
}

struct ScheduleTool {
    schedules: Schedules,
    kind: Kind,
    spec: ToolSpec,
}

pub(crate) fn tools(schedules: Schedules) -> Result<Vec<Arc<dyn Tool>>, SchemaError> {
    Kind::ALL
        .into_iter()
        .map(|kind| {
            Ok(Arc::new(ScheduleTool {
                schedules: schedules.clone(),
                spec: kind.spec()?,
                kind,
            }) as Arc<dyn Tool>)
        })
        .collect()
}

#[async_trait]
impl Tool for ScheduleTool {
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

impl ScheduleTool {
    async fn execute(
        &self,
        session: SessionId,
        args: Map<String, Value>,
    ) -> Result<Value, Failure> {
        match self.kind {
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
