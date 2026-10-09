use crate::{
    attempt::{AttemptKind, PlannedAttempt},
    session::SessionActor,
    snapshot::Snapshot,
    *,
};
use enco_core::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) enum RoundError {
    /// An owner could not commit its state; stop the Session without ending the Round.
    Stopped(KernelError),
    Ended(RoundEnd),
}

impl From<StoreError> for RoundError {
    fn from(e: StoreError) -> Self {
        Self::Stopped(e.into())
    }
}

impl From<RegistryError> for RoundError {
    fn from(e: RegistryError) -> Self {
        Self::Stopped(e.into())
    }
}

pub(crate) fn failed(code: &str, message: impl Into<String>) -> RoundError {
    RoundError::Ended(RoundEnd::Failed {
        failure: Failure {
            code: code.into(),
            message: message.into(),
            retryable: false,
        },
    })
}

pub(crate) fn cancelled(token: &CancellationToken) -> Result<(), RoundError> {
    if token.is_cancelled() {
        Err(RoundError::Ended(RoundEnd::Cancelled))
    } else {
        Ok(())
    }
}

impl SessionActor {
    pub(crate) async fn run(&mut self, token: CancellationToken) -> Result<(), KernelError> {
        let run = RunId::new();
        // The first Round accepts RunStarted and its Inbox inputs in one Commit.
        let mut prefix = vec![EntryBody::RunStarted { run }];
        let mut ending = RunEnd::BudgetExhausted;
        for _ in 0..self.deps.config.max_rounds_per_run {
            let end = self.round(run, &token, std::mem::take(&mut prefix)).await?;
            if let Some(terminal) = end.terminal_run_end() {
                ending = terminal;
                break;
            }
            if end == RoundEnd::Replied
                && self.deps.store.pending(self.session.id).await?.is_empty()
            {
                ending = RunEnd::Completed;
                break;
            }
        }
        self.commit(vec![EntryBody::RunEnded { run, end: ending }], vec![])
            .await?;
        Ok(())
    }

    async fn round(
        &mut self,
        run: RunId,
        token: &CancellationToken,
        mut prefix: Vec<EntryBody>,
    ) -> Result<RoundEnd, KernelError> {
        let round = RoundId::new();
        self.session = self
            .deps
            .store
            .session(self.session.id)
            .await?
            .ok_or(StoreError::UnknownSession(self.session.id))?;
        let snapshot = self.deps.snapshot.clone();
        let safe_mode = self.deps.store.node().await?.safe_mode;
        let pending = self.deps.store.pending(self.session.id).await?;
        let consumed = pending.iter().map(|e| e.id).collect();
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
        let end = match self.execute_round(round, snapshot, safe_mode, token).await {
            Ok(end) | Err(RoundError::Ended(end)) => end,
            Err(RoundError::Stopped(e)) => return Err(e),
        };
        self.commit(
            vec![EntryBody::RoundEnded {
                round,
                end: end.clone(),
            }],
            vec![],
        )
        .await?;
        Ok(end)
    }

    async fn execute_round(
        &mut self,
        round: RoundId,
        snapshot: Arc<Snapshot>,
        safe_mode: bool,
        token: &CancellationToken,
    ) -> Result<RoundEnd, RoundError> {
        let deps = self.deps.clone();
        let profile = deps.profiles.get(&self.session.profile).ok_or_else(|| {
            failed(
                code::PROFILE_UNKNOWN,
                format!("unknown profile {}", self.session.profile),
            )
        })?;
        let planned = self
            .compose(round, &snapshot, profile, safe_mode, token)
            .await?;
        let completion = self
            .attempt(round, &profile.reply, safe_mode, &planned, token)
            .await?;
        for call in completion.message.tool_calls() {
            self.dispatch(round, &snapshot, &planned.plan, call, token)
                .await?;
        }
        cancelled(token)?;
        Ok(if completion.message.tool_calls().next().is_some() {
            RoundEnd::ToolsSettled
        } else {
            RoundEnd::Replied
        })
    }

    async fn compose(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        profile: &Profile,
        safe_mode: bool,
        token: &CancellationToken,
    ) -> Result<PlannedAttempt, RoundError> {
        // Capture external context once. Compaction changes only the Log projection.
        let mut input = self
            .compose_input(snapshot, profile, safe_mode, token)
            .await?;
        loop {
            let composition = snapshot.composer.compose(&input).map_err(|e| {
                failed(
                    if matches!(e, ComposeError::ContextOverflow { .. }) {
                        code::CONTEXT_OVERFLOW
                    } else {
                        code::COMPOSE_FAILED
                    },
                    e.to_string(),
                )
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
            crate::plan::validate(&plan, &input, kind.purpose(), &snapshot.lifeline)
                .map_err(|e| failed(code::PLAN_INVALID, e.to_string()))?;
            let exports = self.deps.registry.exports();
            let target = exports
                .completion(&profile.endpoint(kind.purpose()).plugin, safe_mode)
                .map_err(|failure| RoundError::Ended(RoundEnd::Failed { failure }))?
                .plugin;
            let request = crate::plan::resolve(&plan, &input.transcript, &exports, target)
                .map_err(|e| failed(code::PLAN_INVALID, e.to_string()))?;
            let planned = PlannedAttempt {
                kind,
                plan,
                request,
            };
            if matches!(planned.kind, AttemptKind::Reply) {
                return Ok(planned);
            }
            self.attempt(
                round,
                profile.endpoint(planned.kind.purpose()),
                safe_mode,
                &planned,
                token,
            )
            .await?;
            input.compactions_left -= 1;
            input.transcript = crate::transcript::project(&self.entries);
        }
    }

    async fn compose_input(
        &self,
        snapshot: &Snapshot,
        profile: &Profile,
        safe_mode: bool,
        token: &CancellationToken,
    ) -> Result<ComposeInput, RoundError> {
        let now = self.deps.clock.now();
        let latest_event = self.entries.iter().rev().find_map(|e| match &e.body {
            EntryBody::EventConsumed { event } => Some(event.clone()),
            _ => None,
        });
        let new_input = self
            .entries
            .iter()
            .rev()
            .take_while(|entry| !matches!(entry.body, EntryBody::RoundEnded { .. }))
            .any(|entry| matches!(entry.body, EntryBody::EventConsumed { .. }));
        let query = ContextQuery {
            session: self.session.clone(),
            latest_event,
            new_input,
            cancel: token.child_token(),
        };
        let mut context = Contribution::default();
        if !safe_mode {
            for source in &snapshot.context {
                cancelled(token)?;
                let contribution = source.contribute(&query).await;
                cancelled(token)?;
                let mut contribution =
                    contribution.map_err(|e| failed(code::CONTEXT_FAILED, e.to_string()))?;
                context.candidates.append(&mut contribution.candidates);
                context.omitted.append(&mut contribution.omitted);
            }
        }
        let tools = snapshot
            .tools
            .iter()
            .filter(|t| !safe_mode || snapshot.lifeline.contains(&t.id))
            .map(|t| (t.id.clone(), t.spec.clone()))
            .collect();
        let previous_plan = self.previous_plan(&mut context.omitted).await?;
        Ok(ComposeInput {
            now,
            timezone: self.deps.config.timezone,
            session: self.session.clone(),
            transcript: crate::transcript::project(&self.entries),
            previous_plan,
            context,
            tools,
            safe_mode,
            profile: profile.clone(),
            compactions_left: limits::MAX_COMPACTIONS_PER_ROUND,
        })
    }

    async fn previous_plan(
        &self,
        omitted: &mut Vec<Omission>,
    ) -> Result<Option<ContextPlan>, RoundError> {
        let Some(hash) = self
            .entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.body {
                EntryBody::AttemptStarted {
                    purpose: AttemptPurpose::Reply,
                    plan,
                    ..
                } => Some(plan),
                _ => None,
            })
        else {
            return Ok(None);
        };
        match self.deps.store.get_blob(hash).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
                failed(code::PLAN_INVALID, format!("previous plan {hash}: {error}"))
            }),
            Err(StoreError::Blob(_)) => {
                omitted.push(Omission {
                    source: format!("previous-plan:{hash}"),
                    reason: "previous plan is missing or corrupt".into(),
                });
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
}
