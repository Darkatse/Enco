mod delegation;
mod schedule;
mod settle;
use delegation::DelegationRequest;
use schedule::ScheduleRequest;

use crate::{
    builtin::{BuiltinCall, REPLY_SKIP},
    session::SessionActor,
    snapshot::{Snapshot, ToolSource},
    *,
};
use enco_core::*;
use tokio_util::sync::CancellationToken;

/// What executing a call returned for the actor to settle.
enum Execution {
    /// A tool's outcome.
    Result(Outcome),
    /// A message to a child, checked and delivered with the settlement.
    Delegation(DelegationRequest),
    /// A scheduling call, answered with the settlement and recording any change with it.
    Schedule(ScheduleRequest),
}

pub(crate) fn render_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { value, .. } => match value {
            serde_json::Value::String(s) => s.clone(),
            _ => format!("{value:#}"),
        },
        Outcome::Failed { failure } => format!("error [{}]: {}", failure.code, failure.message),
        Outcome::Unknown { failure } => format!(
            "outcome unknown [{}]: {}. The action may already have taken effect; check the current state before retrying.",
            failure.code, failure.message
        ),
    }
}

impl SessionActor {
    /// The actor alone orders calls and records their facts; workers only return outcomes.
    ///
    /// `timezone` is the one the Round sampled with its configuration.
    pub(crate) async fn dispatch(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        plan: &ContextPlan,
        calls: &[&ToolCall],
        timezone: Tz,
        token: &CancellationToken,
    ) -> Result<(), crate::run::RoundError> {
        let skip = skip_allowed(calls.len(), self.tail.entries());
        let mut remaining = calls;
        while !remaining.is_empty() {
            let count = remaining
                .iter()
                .take_while(|call| {
                    disclosed(snapshot, plan, call)
                        .is_some_and(|tool| tool.spec.effect == Effect::ReadOnly)
                })
                .count()
                .max(1);
            let (batch, rest) = remaining.split_at(count);
            let batch = Batch {
                round,
                snapshot,
                plan,
                calls: batch,
                skip: &skip,
                timezone,
            };
            self.dispatch_batch(&batch, token).await?;
            remaining = rest;
        }
        Ok(())
    }

    async fn dispatch_batch(
        &mut self,
        batch: &Batch<'_>,
        token: &CancellationToken,
    ) -> Result<(), crate::run::RoundError> {
        let Batch {
            round,
            snapshot,
            plan,
            calls,
            skip,
            timezone,
        } = *batch;
        // Only for the health gate: a failure in an earlier batch of this Round counts.
        let view = self.deps.registry.view();
        let mut ready = Vec::new();
        let mut outcomes = Vec::new();
        for (index, call) in calls.iter().enumerate() {
            match prepare(snapshot, plan, call, skip, token, &view.exports) {
                Ok((tool, args)) => ready.push((index, tool, args)),
                Err(failure) => {
                    outcomes.push((index, None, Execution::Result(Outcome::Failed { failure })))
                }
            }
        }
        let starts: Vec<_> = ready
            .iter()
            .map(|(index, tool, _)| EntryBody::ToolCallStarted {
                round,
                call: calls[*index].id,
                capability: tool.id.clone(),
                code: tool.code.clone(),
                effect: tool.spec.effect,
            })
            .collect();
        if !starts.is_empty() {
            self.commit(starts, None).await?;
        }
        let batch_cancel = token.child_token();
        let mut tasks = tokio::task::JoinSet::new();
        let now = self.deps.clock.now();
        for (index, tool, args) in ready {
            let code = tool.code.clone();
            let builtin = tool.builtin();
            let name = self.session.name.clone();
            let tool = tool.tool.clone();
            let ctx = CallContext {
                session: self.session.id,
                working_dir: self.session.working_dir.clone(),
                call: calls[index].id,
                cancel: batch_cancel.child_token(),
                result_budget: limits::TOOL_RESULT_INLINE_BYTES,
            };
            tasks.spawn(async move {
                let parsed = match builtin {
                    Some(BuiltinCall::SessionSend) => {
                        DelegationRequest::parse(args, &name, &ctx.working_dir)
                            .map(Execution::Delegation)
                    }
                    Some(BuiltinCall::Schedule(call)) => {
                        ScheduleRequest::parse(call, args, ctx.session, now, timezone)
                            .map(Execution::Schedule)
                    }
                    Some(BuiltinCall::ReplySkip) | None => {
                        Ok(Execution::Result(tool.call(ctx, args).await))
                    }
                };
                let execution =
                    parsed.unwrap_or_else(|failure| Execution::Result(Outcome::Failed { failure }));
                (index, Some(code), execution)
            });
        }
        let mut stopped = None;
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(result) => outcomes.push(result),
                Err(error) => {
                    batch_cancel.cancel();
                    stopped = Some(KernelError::TaskFailed(format!("tool batch: {error}")));
                }
            }
        }
        if let Some(error) = stopped {
            return Err(crate::run::RoundError::Stopped(error));
        }
        outcomes.sort_unstable_by_key(|(index, _, _)| *index);
        for (index, code, outcome) in outcomes {
            let verdict = match &outcome {
                Execution::Delegation(..) | Execution::Schedule(..) => Verdict::Ok,
                Execution::Result(Outcome::Ok { .. }) => Verdict::Ok,
                Execution::Result(Outcome::Failed { failure } | Outcome::Unknown { failure }) => {
                    Verdict::Failed(failure.clone())
                }
            };
            self.settle(calls[index].id, outcome).await?;
            if let Some(code) = code {
                self.deps
                    .registry
                    .report(&code, verdict, Some(self.session.id))
                    .await?;
            }
        }
        Ok(())
    }

    /// How a Round ends once its calls are settled: a `reply_skip` that settled ok as the reply's
    /// only call leaves the reply unsent.
    pub(crate) fn round_end(
        &self,
        snapshot: &Snapshot,
        plan: &ContextPlan,
        calls: &[&ToolCall],
    ) -> RoundEnd {
        match calls {
            [] => RoundEnd::Replied,
            [call]
                if disclosed(snapshot, plan, call)
                    .is_some_and(|tool| tool.builtin() == Some(BuiltinCall::ReplySkip))
                    && self.settled_ok(call.id) =>
            {
                RoundEnd::ReplySkipped
            }
            _ => RoundEnd::ToolsSettled,
        }
    }

    /// Whether `call`, proposed in this Round, settled successfully.
    fn settled_ok(&self, call: CallId) -> bool {
        crate::round::this_round(self.tail.entries())
            .iter()
            .rev()
            .find_map(|entry| match &entry.body {
                EntryBody::ToolCallSettled {
                    call: settled,
                    outcome,
                    ..
                } if *settled == call => Some(matches!(outcome, Settlement::Ok)),
                _ => None,
            })
            == Some(true)
    }
}

/// One batch of a reply's calls, with what the whole reply shares.
#[derive(Clone, Copy)]
struct Batch<'a> {
    round: RoundId,
    snapshot: &'a Snapshot,
    plan: &'a ContextPlan,
    calls: &'a [&'a ToolCall],
    skip: &'a Result<(), Failure>,
    timezone: Tz,
}

fn disclosed<'a>(
    snapshot: &'a Snapshot,
    plan: &ContextPlan,
    call: &ToolCall,
) -> Option<&'a crate::snapshot::SnapshotTool> {
    let (id, _) = plan.tools.iter().find(|(_, spec)| spec.name == call.name)?;
    snapshot.tools.iter().find(|tool| tool.id == *id)
}

fn prepare<'a>(
    snapshot: &'a Snapshot,
    plan: &ContextPlan,
    call: &ToolCall,
    skip: &Result<(), Failure>,
    token: &CancellationToken,
    exports: &Exports,
) -> Result<
    (
        &'a crate::snapshot::SnapshotTool,
        serde_json::Map<String, serde_json::Value>,
    ),
    Failure,
> {
    let failure = |code: &str, message: String| Failure {
        code: code.into(),
        message,
        retryable: false,
    };
    if token.is_cancelled() {
        return Err(failure(
            code::CANCELLED,
            "not executed: the run was cancelled before this call".into(),
        ));
    }
    let tool = disclosed(snapshot, plan, call).ok_or_else(|| {
        failure(
            code::TOOL_UNAVAILABLE,
            format!("tool `{}` is not available in this Round", call.name),
        )
    })?;
    if let ToolSource::Plugin(name) = &tool.source
        && let Some(failed) = exports.failed(&tool.code)
    {
        return Err(failure(
            code::PLUGIN_ROLLED_BACK,
            rolled_back(name, &failed),
        ));
    }
    let args = match Arguments::parse(&call.arguments) {
        Arguments::Object(args) => args,
        Arguments::Invalid { raw } => {
            return Err(failure(
                code::TOOL_INVALID_ARGUMENTS,
                format!(
                    "arguments must be a JSON object; received: {}",
                    raw.chars().take(200).collect::<String>()
                ),
            ));
        }
    };
    let args = tool.spec.input_schema.accept(args).map_err(|error| {
        failure(
            code::TOOL_INVALID_ARGUMENTS,
            format!("invalid arguments for `{}`: {error}", call.name),
        )
    })?;
    if tool.builtin() == Some(BuiltinCall::ReplySkip) {
        skip.clone()?;
    }
    Ok((tool, args))
}

/// Whether `reply_skip` may end a reply proposing `calls` calls: only as the sole call, and while
/// no interactive input awaits an answer.
///
/// Both are facts of the whole reply, known before any call runs, so a refused skip never reaches
/// its tool.
fn skip_allowed(calls: usize, entries: &[Entry]) -> Result<(), Failure> {
    let message = if calls > 1 {
        format!(
            "not executed: {REPLY_SKIP} must be the only call in its turn; call it alone after your other work"
        )
    } else if unanswered_input(entries).is_some() {
        "not executed: someone is waiting for your reply; reply instead".into()
    } else {
        return Ok(());
    };
    Err(Failure {
        code: code::TOOL_REFUSED.into(),
        message,
        retryable: false,
    })
}

/// A call bound to code that has failed: what happened, and what calls from the next Round
/// reach, since this Round's catalog stays bound to the failed code.
fn rolled_back(plugin: &PluginName, failed: &Failed<'_>) -> String {
    let cause = format!(
        "not executed: {plugin} failed with registration {} ({}: {})",
        failed.from.registration, failed.failure.code, failed.failure.message
    );
    match failed.current {
        Some(code) => {
            format!(
                "{cause} and was rolled back to {code}; calls from the next Round go to {code}."
            )
        }
        None => format!(
            "{cause}; nothing earlier can run, so its tools are unavailable from the next Round."
        ),
    }
}
