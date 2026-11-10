//! The scheduling calls read and change only their own Session's schedules, and change them only
//! with their settlement.
//!
//! Executing one parses and validates it; the actor answers it from the Session's effective
//! schedules in call order, so a call sees what earlier calls of the same reply changed.
use super::*;
use crate::{
    Scheduled,
    builtin::{CancelArguments, CreateArguments, ScheduleCall},
};

pub(super) enum ScheduleRequest {
    /// A valid new schedule, its first occurrence derived from its creation time.
    Create(Scheduled),
    /// The active schedules, with occurrences in the Round's time zone.
    List(Tz),
    /// A schedule the call names; whether it is this Session's and active is checked at
    /// settlement.
    Cancel(ScheduleId),
}

impl ScheduleRequest {
    /// Parse a scheduling call as of `now`, in the time zone its Round sampled.
    pub fn parse(
        call: ScheduleCall,
        args: serde_json::Map<String, serde_json::Value>,
        session: SessionId,
        now: DateTime<Utc>,
        timezone: Tz,
    ) -> Result<Self, Failure> {
        match call {
            ScheduleCall::Create => {
                let arguments: CreateArguments = parse_arguments(args)?;
                let schedule = Schedule {
                    id: ScheduleId::new(),
                    session,
                    rule: rule(&arguments)?,
                    message: arguments.message,
                    created_at: now,
                    state: ScheduleState::Active,
                    last: None,
                };
                Scheduled::create(schedule, timezone)
                    .map(Self::Create)
                    .map_err(schedule_failure)
            }
            ScheduleCall::List => Ok(Self::List(timezone)),
            ScheduleCall::Cancel => {
                let CancelArguments { id } = parse_arguments(args)?;
                Ok(Self::Cancel(id))
            }
        }
    }
}

impl SessionActor {
    /// What a scheduling call answers and the change it records, or why it changes nothing.
    ///
    /// Earlier calls of the same reply have committed, so their changes are in the view read here.
    /// A cancel or reminder accepted between this read and the commit only makes a cancel
    /// redundant: the schedules fold by identity, and cancelling twice is cancelling once.
    pub(super) async fn schedule_call(
        &self,
        request: ScheduleRequest,
    ) -> Result<Result<(Option<ScheduleChange>, serde_json::Value), Failure>, StoreError> {
        Ok(match request {
            ScheduleRequest::Create(scheduled) => {
                let schedule = &scheduled.schedule;
                let change = ScheduleChange::Created {
                    id: schedule.id,
                    rule: schedule.rule.clone(),
                    message: schedule.message.clone(),
                    created_at: schedule.created_at,
                };
                Ok((Some(change), scheduled.into_json()))
            }
            ScheduleRequest::List(timezone) => {
                crate::scheduler::in_due_order(self.schedules().await?, timezone)
                    .map(|scheduled| {
                        let listed = scheduled.into_iter().map(Scheduled::into_json).collect();
                        (None, serde_json::Value::Array(listed))
                    })
                    .map_err(schedule_failure)
            }
            ScheduleRequest::Cancel(id) => {
                let active =
                    self.schedules().await?.into_iter().any(|schedule| {
                        schedule.id == id && schedule.state == ScheduleState::Active
                    });
                if active {
                    Ok((
                        Some(ScheduleChange::Cancelled { id }),
                        format!("Cancelled schedule {id}.").into(),
                    ))
                } else {
                    Err(Failure {
                        code: code::TOOL_FAILED.into(),
                        message: format!("{id} is not one of this Session's active schedules"),
                        retryable: false,
                    })
                }
            }
        })
    }

    /// This Session's schedules in its effective view.
    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        self.deps.store.schedules(Some(self.session.id)).await
    }
}

fn rule(arguments: &CreateArguments) -> Result<ScheduleRule, Failure> {
    match (&arguments.at, &arguments.cron) {
        (Some(at), None) => {
            if arguments.timezone.is_some() {
                return Err(invalid("timezone only applies to cron"));
            }
            let at = DateTime::parse_from_rfc3339(at)
                .map_err(|e| invalid(format!("at must include a UTC offset: {e}")))?
                .to_utc();
            Ok(ScheduleRule::Once { at })
        }
        (None, Some(cron)) => Ok(ScheduleRule::Cron {
            expr: cron.clone(),
            timezone: arguments
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

fn invalid(message: impl Into<String>) -> Failure {
    Failure {
        code: code::TOOL_INVALID_ARGUMENTS.into(),
        message: message.into(),
        retryable: false,
    }
}

/// A scheduling failure as the model reads it: the rule or text is invalid, or reading failed.
fn schedule_failure(error: ScheduleError) -> Failure {
    let code = match error {
        ScheduleError::EmptyMessage
        | ScheduleError::InvalidCron(_)
        | ScheduleError::NoOccurrenceAfter(_)
        | ScheduleError::TooFrequent => code::TOOL_INVALID_ARGUMENTS,
        ScheduleError::Restore { .. } | ScheduleError::Store(_) => code::TOOL_FAILED,
    };
    Failure {
        code: code.into(),
        message: error.to_string(),
        retryable: false,
    }
}
