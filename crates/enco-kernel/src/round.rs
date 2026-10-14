mod context;
mod selection;

use crate::{
    attempt::{AttemptKind, PlannedAttempt},
    run::{RoundError, cancelled, failed},
    session::SessionActor,
    snapshot::Snapshot,
    *,
};
use enco_core::*;
use selection::{Choice, Selection};
use tokio_util::sync::CancellationToken;

impl SessionActor {
    /// Run and settle one Round; returns its end and whether it ran in safe mode, from which the
    /// Run decides whether to continue.
    pub(crate) async fn round(
        &mut self,
        run: RunId,
        token: &CancellationToken,
        mut prefix: Vec<EntryBody>,
    ) -> Result<(RoundEnd, bool), KernelError> {
        let round = RoundId::new();
        let (session, view) = self.deps.registry.session_view(self.session.id).await?;
        self.session = session;
        let mode = safe_mode(
            &self.entries,
            run,
            self.deps.store.node().await?.safe_mode,
            &view.active_generations(),
        );
        let safe_mode = mode.is_some();
        let pending = self.deps.store.pending(self.session.id).await?;
        let consumed = pending.iter().map(|event| event.id).collect();
        prefix.extend(
            pending
                .into_iter()
                .map(|event| EntryBody::EventConsumed { event }),
        );
        prefix.push(EntryBody::RoundStarted {
            run,
            round,
            safe_mode,
        });
        self.commit(prefix, consumed).await?;
        let end = match self.execute_round(round, &view, mode, token).await {
            Ok(end) => end,
            Err(RoundError::Cancelled) => RoundEnd::Cancelled,
            // Health reports have settled, so an automatic rollback counts before this failure.
            Err(RoundError::Failed(failure)) => RoundEnd::Failed {
                failure,
                generations: self.deps.registry.view().active_generations(),
            },
            Err(RoundError::Stopped(error)) => return Err(error),
        };
        self.commit(
            vec![EntryBody::RoundEnded {
                round,
                end: end.clone(),
            }],
            vec![],
        )
        .await?;
        Ok((end, safe_mode))
    }

    async fn execute_round(
        &mut self,
        round: RoundId,
        view: &View,
        mode: Option<SafeMode>,
        token: &CancellationToken,
    ) -> Result<RoundEnd, RoundError> {
        let snapshot = self.deps.snapshot.for_round(&view.exports, mode.is_some());
        let settings = &view.config.kernel;
        let profile = settings
            .profiles
            .get(&self.session.profile)
            .ok_or_else(|| {
                failed(
                    code::PROFILE_UNKNOWN,
                    format!("unknown profile {}", self.session.profile),
                )
            })?;
        let safe_mode = mode.is_some();
        let mut selection = self.select(profile, safe_mode);
        let reply = selection.reply_target()?;
        // Context sources run once; a later composition changes only its targets and Transcript.
        let mut input = self
            .compose_input(
                &snapshot,
                settings.timezone,
                mode,
                reply,
                selection.compaction_budget(),
                token,
            )
            .await?;
        loop {
            let (plan, kind) = compose(&snapshot, &input, profile.requires_lifeline, &selection)?;
            let choice = selection.choose(kind.purpose())?;
            let planned = self
                .freeze(&snapshot, plan, kind, &input.transcript, &selection, choice)
                .await?;
            let endpoint = &choice.endpoint.endpoint;
            let completion = match self
                .attempt(round, endpoint, &choice.export, safe_mode, &planned, token)
                .await
            {
                Ok(completion) => completion,
                // The failed combination is exhausted in the Log now, so selecting again moves on.
                Err(RoundError::Failed(_)) => {
                    selection = self.retarget(profile, safe_mode, &mut input)?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let AttemptKind::Compaction(_) = planned.kind {
                input.compactions_left -= 1;
                input.transcript = crate::transcript::project(&self.entries);
                selection = self.retarget(profile, safe_mode, &mut input)?;
                continue;
            }
            let calls: Vec<_> = completion.message.tool_calls().collect();
            self.dispatch(round, &snapshot, &planned.plan, &calls, token)
                .await?;
            cancelled(token)?;
            return Ok(if completion.message.tool_calls().next().is_some() {
                RoundEnd::ToolsSettled
            } else {
                RoundEnd::Replied
            });
        }
    }

    /// Select against the latest view and Log; the Round fails once no reply candidate is usable.
    fn retarget<'p>(
        &self,
        profile: &'p Profile,
        safe_mode: bool,
        input: &mut ComposeInput,
    ) -> Result<Selection<'p>, RoundError> {
        let selection = self.select(profile, safe_mode);
        input.reply = selection.reply_target()?;
        input.compaction = selection.compaction_budget();
        Ok(selection)
    }

    /// Resolve the request for the chosen code and record the plan it was resolved from.
    async fn freeze(
        &self,
        snapshot: &Snapshot,
        plan: ContextPlan,
        kind: AttemptKind,
        transcript: &Transcript,
        selection: &Selection<'_>,
        choice: &Choice<'_>,
    ) -> Result<PlannedAttempt, RoundError> {
        let request =
            crate::plan::resolve(&plan, transcript, selection.exports(), choice.export.plugin)
                .map_err(|error| failed(code::PLAN_INVALID, error.to_string()))?;
        let bytes = serde_json::to_vec(&plan)
            .map_err(|error| failed(code::PLAN_INVALID, error.to_string()))?;
        let hash = self.deps.store.put_blob(&bytes).await?;
        Ok(PlannedAttempt {
            kind,
            composer: snapshot.composer.code(),
            plan,
            hash,
            request,
        })
    }
}

/// Ask the composer for a plan and validate it; the plan's kind decides the purpose it serves.
fn compose(
    snapshot: &Snapshot,
    input: &ComposeInput,
    requires_lifeline: bool,
    selection: &Selection<'_>,
) -> Result<(ContextPlan, AttemptKind), RoundError> {
    let composition = snapshot
        .composer
        .compose(input)
        .map_err(|error| match error {
            // The kernel's own record says why compaction is unusable.
            ComposeError::CompactionUnavailable => {
                match selection.choose(AttemptPurpose::Compaction) {
                    Err(unusable) => unusable,
                    Ok(_) => failed(code::COMPOSE_FAILED, error.to_string()),
                }
            }
            ComposeError::ContextOverflow { .. } => {
                failed(code::CONTEXT_OVERFLOW, error.to_string())
            }
            ComposeError::Invalid(_) => failed(code::COMPOSE_FAILED, error.to_string()),
        })?;
    let (plan, kind) = match composition {
        Composition::Plan(plan) => (plan, AttemptKind::Reply),
        Composition::Compact { upto, plan } => {
            if input.compactions_left == 0 {
                return Err(failed(
                    code::COMPOSE_FAILED,
                    "too many compactions in one Round",
                ));
            }
            if !input.transcript.round_ends.contains(&upto) {
                return Err(failed(
                    code::PLAN_INVALID,
                    "compaction must end at a complete Round",
                ));
            }
            (plan, AttemptKind::Compaction(upto))
        }
    };
    crate::plan::validate(
        &plan,
        input,
        kind.purpose(),
        &snapshot.lifeline,
        requires_lifeline,
    )
    .map_err(|error| failed(code::PLAN_INVALID, error.to_string()))?;
    Ok((plan, kind))
}

/// Safe mode follows the owner's switch, or this Run's latest failed Round while the registered
/// behavior it failed against is still active.
fn safe_mode(
    entries: &[Entry],
    run: RunId,
    switch: bool,
    active: &[GenerationId],
) -> Option<SafeMode> {
    if switch {
        return Some(SafeMode::Owner);
    }
    let entries = run_entries(entries);
    // Until its first Round commits, the latest RunStarted belongs to the previous Run.
    if !matches!(entries.first().map(|entry| &entry.body), Some(EntryBody::RunStarted { run: started }) if *started == run)
    {
        return None;
    }
    let (failure, generations) = entries.iter().rev().find_map(|entry| match &entry.body {
        EntryBody::RoundEnded {
            end:
                RoundEnd::Failed {
                    failure,
                    generations,
                },
            ..
        } => Some((failure, generations)),
        _ => None,
    })?;
    (generations == active).then(|| SafeMode::Failed(failure.clone()))
}

/// Recovery decisions use the current Run's facts, including entries hidden by compaction.
fn run_entries(entries: &[Entry]) -> &[Entry] {
    entries
        .iter()
        .rposition(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
        .map_or(&[], |start| &entries[start..])
}
