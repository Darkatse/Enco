mod context;
mod delegation;
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
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl SessionActor {
    /// Run and settle one Round that consumes `inputs` as it starts; returns its end and whether it
    /// ran in safe mode, from which the Run decides whether to continue.
    pub(crate) async fn round(
        &mut self,
        run: RunId,
        token: &CancellationToken,
        mut prefix: Vec<EntryBody>,
        inputs: Vec<Event>,
    ) -> Result<(RoundEnd, bool), KernelError> {
        let round = RoundId::new();
        let (session, ancestors, view) = self.deps.registry.session_view(self.session.id).await?;
        self.session = session;
        let mode = safe_mode(
            &self.entries,
            run,
            self.deps.store.node().await?.safe_mode,
            &view.active_generations(),
        );
        let safe_mode = mode.is_some();
        prefix.extend(
            inputs
                .into_iter()
                .map(|event| EntryBody::EventConsumed { event }),
        );
        prefix.push(EntryBody::RoundStarted {
            run,
            round,
            safe_mode,
            working_dir: self.session.working_dir.clone(),
        });
        self.commit(prefix, None).await?;
        let end = match self
            .execute_round(round, &ancestors, &view, mode, token)
            .await
        {
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
            None,
        )
        .await?;
        Ok((end, safe_mode))
    }

    async fn execute_round(
        &mut self,
        round: RoundId,
        ancestors: &[SessionRecord],
        view: &View,
        mode: Option<SafeMode>,
        token: &CancellationToken,
    ) -> Result<RoundEnd, RoundError> {
        let snapshot = self.deps.snapshot.for_round(&view.exports, mode.is_some());
        let settings = &view.config.kernel;
        let (
            profile,
            delegation::Permissions {
                tools,
                owner_context,
            },
        ) = self.permissions(settings, &snapshot, ancestors)?;
        let safe_mode = mode.is_some();
        let selection = self.select(profile, safe_mode);
        // Fail before gathering context when no reply model is usable.
        selection.choose(AttemptPurpose::Reply)?;
        let now = self.deps.clock.now();
        let gathered = self
            .gather(&snapshot, safe_mode, owner_context, token)
            .await?;
        let (reply, compaction) = self
            .targets(&selection, gathered.measured.as_ref(), &gathered.transcript)
            .await?;
        let input = ComposeInput {
            round,
            now,
            home: self.deps.home.clone(),
            timezone: settings.timezone,
            session: self.session.clone(),
            children: self.children().await?,
            profiles: if !safe_mode && tools.contains(crate::builtin::SESSION_SEND) {
                delegation::profiles(settings)
            } else {
                Vec::new()
            },
            transcript: gathered.transcript,
            blob_paths: gathered.blob_paths,
            previous_plan: gathered.previous_plan,
            compaction_requested: gathered.compaction_requested,
            owner_context,
            owner_instructions: gathered.owner_instructions,
            profile_instructions: gathered.profile_instructions,
            memories: gathered.memories,
            omitted: gathered.omitted,
            tools: snapshot
                .tools
                .iter()
                .filter(|tool| !safe_mode || snapshot.lifeline.contains(&tool.id))
                .filter(|tool| tools.contains(&tool.id.name))
                .map(|tool| (tool.id.clone(), tool.spec.clone()))
                .collect(),
            lifeline: if profile.requires_lifeline {
                snapshot
                    .lifeline
                    .iter()
                    .filter(|id| tools.contains(&id.name))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            },
            unavailable: snapshot.unavailable.clone(),
            safe_mode: mode,
            reply,
            compaction,
        };
        let targeting = Targeting {
            profile,
            safe_mode,
            measured: gathered.measured,
        };
        self.answer(round, &snapshot, &targeting, input, selection, token)
            .await
    }

    /// Compose, attempt and settle until the Round answers.
    ///
    /// A failed combination moves on to the next candidate, a compaction recomposes over the new
    /// transcript, and a refusal as too large recomposes for the same combination, whose targets
    /// now carry the refused plan.
    ///
    /// The loop ends: a compaction ends at a Round end the transcript shows and hides it, and no
    /// Round ends are recorded within a Round; a failure exhausts its combination for the Run; and
    /// refusals, which need not make a request smaller, are bounded.
    async fn answer<'p>(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        targeting: &Targeting<'p>,
        mut input: ComposeInput,
        mut selection: Selection<'p>,
        token: &CancellationToken,
    ) -> Result<RoundEnd, RoundError> {
        let mut overflows = 0;
        loop {
            let (plan, kind, composer) = self
                .compose(targeting.profile, &input, &selection, token)
                .await?;
            let choice = selection.choose(kind.purpose())?;
            let planned = self
                .freeze(plan, kind, composer, &input.transcript, &selection, choice)
                .await?;
            let endpoint = &choice.endpoint.endpoint;
            let completion = match self
                .attempt(
                    round,
                    endpoint,
                    &choice.export,
                    targeting.safe_mode,
                    &planned,
                    token,
                )
                .await
            {
                Ok(Ok(completion)) => completion,
                // The refusal is in the Log now, so the targets its combination serves carry it.
                Ok(Err(overflow)) => {
                    overflows += 1;
                    if overflows > limits::MAX_OVERFLOW_RESENDS {
                        return Err(RoundError::Failed(overflow.failure()));
                    }
                    selection = self.retarget(targeting, &mut input).await?;
                    continue;
                }
                // The failed combination is exhausted in the Log now, so selecting again moves on.
                Err(RoundError::Failed(_)) => {
                    selection = self.retarget(targeting, &mut input).await?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let AttemptKind::Compaction(_) = planned.kind {
                (
                    input.transcript,
                    input.blob_paths,
                    input.compaction_requested,
                ) = self.transcript();
                selection = self.retarget(targeting, &mut input).await?;
                continue;
            }
            let calls: Vec<_> = completion.message.tool_calls().collect();
            self.dispatch(round, snapshot, planned.plan.plan(), &calls, token)
                .await?;
            cancelled(token)?;
            return Ok(self.round_end(snapshot, planned.plan.plan(), &calls));
        }
    }

    /// Select against the latest view and Log and build the targets afresh; the Round fails once
    /// no reply candidate is usable.
    async fn retarget<'p>(
        &self,
        targeting: &Targeting<'p>,
        input: &mut ComposeInput,
    ) -> Result<Selection<'p>, RoundError> {
        let selection = self.select(targeting.profile, targeting.safe_mode);
        (input.reply, input.compaction) = self
            .targets(&selection, targeting.measured.as_ref(), &input.transcript)
            .await?;
        Ok(selection)
    }

    /// Resolve the request for the chosen code and record the plan it was resolved from.
    async fn freeze(
        &self,
        plan: ContextPlan,
        kind: AttemptKind,
        composer: GenerationId,
        transcript: &Transcript,
        selection: &Selection<'_>,
        choice: &Choice<'_>,
    ) -> Result<PlannedAttempt, RoundError> {
        let request =
            crate::plan::resolve(&plan, transcript, selection.exports(), choice.export.plugin)
                .map_err(|error| failed(code::PLAN_INVALID, error.to_string()))?;
        let request = crate::image::resolve(request, self.deps.store.as_ref())
            .await
            .map_err(|error| failed(code::CONTEXT_FAILED, format!("image content: {error}")))?;
        Ok(PlannedAttempt {
            kind,
            composer,
            plan: Arc::new(PlanWrite::new(plan)?),
            request,
        })
    }
}

impl SessionActor {
    /// Bind, invoke and check one composition before recording a model attempt.
    async fn compose(
        &self,
        profile: &Profile,
        input: &ComposeInput,
        selection: &Selection<'_>,
        token: &CancellationToken,
    ) -> Result<(ContextPlan, AttemptKind, GenerationId), RoundError> {
        let view = self.deps.registry.view();
        let bound = view
            .exports
            .composer(&profile.composer, input.safe_mode.is_some())
            .map_err(RoundError::Failed)?;
        let composed = tokio::select! {
            biased;
            _ = token.cancelled() => return Err(RoundError::Cancelled),
            result = bound.export.adapter.compose(&bound.options, input) => result,
        };
        let checked = composed.and_then(|composition| bind_composition(composition, input));
        let verdict = match &checked {
            Ok(_) => Verdict::Ok,
            Err(failure) => Verdict::Failed(failure.clone()),
        };
        self.deps
            .registry
            .report(bound.export.generation, verdict, Some(self.session.id))
            .await?;
        checked
            .map(|(plan, kind)| (plan, kind, bound.export.generation))
            .map_err(|mut failure| {
                if failure.code == code::CONTEXT_OVERFLOW
                    && input.compaction.is_none()
                    && let Err(RoundError::Failed(reason)) =
                        selection.choose(AttemptPurpose::Compaction)
                {
                    failure
                        .message
                        .push_str(&format!("; compaction unavailable: {}", reason.message));
                }
                RoundError::Failed(failure)
            })
    }
}

/// What choosing a Round's targets again needs.
struct Targeting<'p> {
    /// The profile whose candidates are chosen from.
    profile: &'p Profile,
    /// Whether the Round runs in safe mode.
    safe_mode: bool,
    /// What the service measured for the previous plan.
    measured: Option<context::Measured>,
}

fn bind_composition(
    composition: Composition,
    input: &ComposeInput,
) -> Result<(ContextPlan, AttemptKind), Failure> {
    let invalid = |message: String| Failure {
        code: code::PLUGIN_CONTRACT.into(),
        message,
        retryable: false,
    };
    let (plan, kind) = match composition {
        Composition::Plan(plan) => (plan, AttemptKind::Reply),
        Composition::Compact { upto, plan } => {
            if input.compaction.is_none() {
                return Err(invalid(
                    "compaction is not available for this composition".into(),
                ));
            }
            if !input.transcript.round_ends.contains(&upto) {
                return Err(invalid("compaction must end at a complete Round".into()));
            }
            // A reply request needs history after the summary to answer.
            if !input
                .transcript
                .items
                .last()
                .is_some_and(|last| last.pos > upto)
            {
                return Err(invalid(
                    "compaction must leave history after it to reply to".into(),
                ));
            }
            (plan, AttemptKind::Compaction(upto))
        }
    };
    let plan = crate::plan::bind(plan, input, kind.purpose())
        .map_err(|error| invalid(error.to_string()))?;
    let overflowed = match kind {
        AttemptKind::Reply => input.reply.overflowed.as_ref(),
        AttemptKind::Compaction(_) => input
            .compaction
            .as_ref()
            .and_then(|compaction| compaction.overflowed.as_ref()),
    };
    // A pure composition sent again would be turned away again.
    if overflowed.is_some_and(|overflowed| overflowed.plan == plan) {
        return Err(invalid(
            "composition repeats the plan its target turned away as too large".into(),
        ));
    }
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
