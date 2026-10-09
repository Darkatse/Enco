use crate::{CallContext, ScheduleError, Scheduled, Schedules, Tool, limits};
use async_trait::async_trait;
use enco_core::*;
use serde_json::{Map, Value, json};
use std::sync::Arc;
enum Kind {
    Create,
    List,
    Cancel,
}

struct ScheduleTool {
    schedules: Schedules,
    kind: Kind,
}

pub(crate) fn tools(schedules: Schedules) -> Vec<Arc<dyn Tool>> {
    [Kind::Create, Kind::List, Kind::Cancel]
        .into_iter()
        .map(|kind| {
            Arc::new(ScheduleTool {
                schedules: schedules.clone(),
                kind,
            }) as Arc<dyn Tool>
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
        let (name, description, effect, properties, required) = match self.kind {
            Kind::Create => (
                "schedule_create",
                format!("Create a reminder delivered to this Session, even if Enco restarts. Give either \
                    at (a future RFC 3339 timestamp with a UTC offset) or cron (minute hour day month weekday). \
                    A cron rule follows the owner's time zone unless timezone names another IANA zone; \
                    occurrences must be at least {} minutes apart. If Enco was not running, only the latest \
                    missed occurrence is delivered.", limits::MIN_RECURRENCE_INTERVAL.as_secs() / 60),
                Effect::SideEffect,
                json!({
                    "at": { "type": "string" },
                    "cron": { "type": "string" },
                    "timezone": { "type": "string" },
                    "message": { "type": "string" },
                }),
                vec!["message"],
            ),
            Kind::List => (
                "schedule_list",
                "List active schedules addressed to this Session, including their rules and next occurrences.".into(),
                Effect::ReadOnly,
                json!({}),
                vec![],
            ),
            Kind::Cancel => (
                "schedule_cancel",
                "Stop future occurrences of a schedule by id. Already delivered reminders are unaffected. \
                 Returns false when it is done, cancelled, or absent.".into(),
                Effect::SideEffect,
                json!({ "id": { "type": "string" } }),
                vec!["id"],
            ),
        };
        ToolSpec {
            name: name.into(),
            description,
            effect,
            input_schema: closed_object_schema(properties, &required),
        }
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.execute(ctx.session, args).await {
            Ok(value) => Outcome::Ok { value },
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
                let rule = match (args.contains_key("at"), args.contains_key("cron")) {
                    (true, false) => {
                        if args.contains_key("timezone") {
                            return Err(invalid("timezone only applies to cron"));
                        }
                        let at = DateTime::parse_from_rfc3339(text(&args, "at")?)
                            .map_err(|e| invalid(format!("at must include a UTC offset: {e}")))?
                            .to_utc();
                        ScheduleRule::Once { at }
                    }
                    (false, true) => {
                        let timezone = args
                            .get("timezone")
                            .map(|_| {
                                text(&args, "timezone")?.parse::<Tz>().map_err(|e| {
                                    invalid(format!("timezone must be an IANA name: {e}"))
                                })
                            })
                            .transpose()?;
                        ScheduleRule::Cron {
                            expr: text(&args, "cron")?.into(),
                            timezone,
                        }
                    }
                    _ => return Err(invalid("give exactly one of at or cron")),
                };
                let scheduled = self
                    .schedules
                    .create(session, rule, text(&args, "message")?.into())
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
                let id = text(&args, "id")?
                    .parse()
                    .map_err(|e| invalid(format!("id: {e}")))?;
                let cancelled = self.schedules.cancel(id).await.map_err(error)?;
                Ok(json!({ "cancelled": cancelled }))
            }
        }
    }
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, Failure> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{key} must be a string")))
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
