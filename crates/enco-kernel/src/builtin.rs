use crate::{CallContext, ScheduleError, Schedules, Tool};
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
                "Create a reminder delivered to this Session at the given time, even if Enco restarts. at \
                 must be a future RFC 3339 timestamp including a UTC offset.",
                Effect::SideEffect,
                json!({
                    "at": { "type": "string" },
                    "message": { "type": "string" },
                }),
                vec!["at", "message"],
            ),
            Kind::List => (
                "schedule_list",
                "List pending reminders addressed to this Session.",
                Effect::ReadOnly,
                json!({}),
                vec![],
            ),
            Kind::Cancel => (
                "schedule_cancel",
                "Cancel a pending reminder by id. Returns false when it has already fired, was cancelled, \
                 or does not exist.",
                Effect::SideEffect,
                json!({ "id": { "type": "string" } }),
                vec!["id"],
            ),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
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
                let due = DateTime::parse_from_rfc3339(text(&args, "at")?)
                    .map_err(|e| invalid(format!("at must include a UTC offset: {e}")))?
                    .to_utc();
                let schedule = self
                    .schedules
                    .create(session, due, text(&args, "message")?.into())
                    .await
                    .map_err(error)?;
                Ok(json!({ "id": schedule.id, "due_at": schedule.due_at }))
            }
            Kind::List => {
                let schedules = self.schedules.list(Some(session)).await.map_err(error)?;
                let reminders: Vec<_> = schedules
                    .into_iter()
                    .map(|schedule| {
                        json!({
                            "id": schedule.id,
                            "due_at": schedule.due_at,
                            "message": schedule.message,
                        })
                    })
                    .collect();
                Ok(json!(reminders))
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
        ScheduleError::InPast(_)
        | ScheduleError::EmptyMessage
        | ScheduleError::UnknownSession(_) => code::TOOL_INVALID_ARGUMENTS,
        _ => code::TOOL_FAILED,
    };
    Failure {
        code: code.into(),
        message: error.to_string(),
        retryable: false,
    }
}
