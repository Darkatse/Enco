use crate::{session::SessionActor, snapshot::Snapshot, *};
use enco_core::*;
use tokio_util::sync::CancellationToken;

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
    async fn settle(&mut self, call: CallId, mut outcome: Outcome) -> Result<(), StoreError> {
        let mut stored = Vec::new();
        if let Outcome::Ok { images, .. } = &outcome {
            for bytes in images {
                match crate::image::store(self.deps.store.as_ref(), bytes).await {
                    Ok(image) => stored.push(image),
                    Err(crate::ImageError::Store(error)) => return Err(error),
                    Err(error) => {
                        outcome = Outcome::Failed {
                            failure: Failure {
                                code: code::TOOL_FAILED.into(),
                                message: error.to_string(),
                                retryable: false,
                            },
                        };
                        stored.clear();
                        break;
                    }
                }
            }
        }
        let outgoing = if let Outcome::Ok { outgoing, .. } = &mut outcome
            && let Some(outgoing) = outgoing.take()
        {
            Some(crate::outgoing::store(self.deps.store.as_ref(), outgoing).await?)
        } else {
            None
        };
        let mut content = render_text(&outcome);
        let mut full = None;
        if content.len() > limits::TOOL_RESULT_INLINE_BYTES {
            let hash = self.deps.store.put_blob(content.as_bytes()).await?;
            let size = content.len();
            content.truncate(content.floor_char_boundary(limits::TOOL_RESULT_PREVIEW_BYTES));
            let path = self.deps.store.blob_path(&hash);
            content.push_str(&format!(
                "\n[truncated: {size} bytes; full result: {}; it may be cleaned up later]",
                path.display(),
            ));
            full = Some(hash);
        }
        for image in &stored {
            content.push_str(&format!(
                "\n[image: {}, {} bytes; file: {}]",
                image.format.mime(),
                image.size,
                self.deps.store.blob_path(&image.content).display()
            ));
        }
        self.commit(vec![EntryBody::ToolCallSettled {
            call,
            outcome: Settlement::from(&outcome),
            content,
            full,
            images: stored,
            outgoing,
        }])
        .await
    }

    /// The actor alone orders calls and records their facts; workers only return outcomes.
    pub(crate) async fn dispatch(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        plan: &ContextPlan,
        calls: &[&ToolCall],
        token: &CancellationToken,
    ) -> Result<(), crate::run::RoundError> {
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
            self.dispatch_batch(round, snapshot, plan, batch, token)
                .await?;
            remaining = rest;
        }
        Ok(())
    }

    async fn dispatch_batch(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        plan: &ContextPlan,
        calls: &[&ToolCall],
        token: &CancellationToken,
    ) -> Result<(), crate::run::RoundError> {
        let view = self.deps.registry.view();
        let mut ready = Vec::new();
        let mut outcomes = Vec::new();
        for (index, call) in calls.iter().enumerate() {
            match prepare(snapshot, plan, call, token, &view.exports) {
                Ok((tool, args)) => ready.push((index, tool, args)),
                Err(failure) => outcomes.push((index, None, Outcome::Failed { failure })),
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
            self.commit(starts).await?;
        }
        let batch_cancel = token.child_token();
        let mut tasks = tokio::task::JoinSet::new();
        for (index, tool, args) in ready {
            let code = tool.code.clone();
            let tool = tool.tool.clone();
            let ctx = CallContext {
                session: self.session.id,
                call: calls[index].id,
                cancel: batch_cancel.child_token(),
                result_budget: limits::TOOL_RESULT_INLINE_BYTES,
            };
            tasks.spawn(async move { (index, Some(code), tool.call(ctx, args).await) });
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
                Outcome::Ok { .. } => Verdict::Ok,
                Outcome::Failed { failure } | Outcome::Unknown { failure } => {
                    Verdict::Failed(failure.clone())
                }
            };
            self.settle(calls[index].id, outcome).await?;
            if let Some(CodeRef::Generation { id }) = code {
                self.deps
                    .registry
                    .report(id, verdict, Some(self.session.id))
                    .await?;
            }
        }
        Ok(())
    }
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
    if let CodeRef::Generation { id } = tool.code
        && let Some((recorded, current)) = exports.failed(id)
    {
        let current = current.map_or_else(|| "none".into(), |id| id.to_string());
        return Err(failure(
            code::PLUGIN_ROLLED_BACK,
            format!(
                "generation {id} was marked failed ({}: {}); current active generation: {current}",
                recorded.code, recorded.message
            ),
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
    Ok((tool, args))
}
