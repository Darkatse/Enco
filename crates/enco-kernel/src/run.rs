use crate::{attempt::AttemptKind, session::SessionActor, snapshot::Snapshot, *};
use enco_core::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) enum RoundError {
    Store(StoreError),
    Ended(RoundEnd),
}

impl From<StoreError> for RoundError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
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
    pub(crate) async fn run(&mut self, token: CancellationToken) -> Result<(), StoreError> {
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
            .await
    }

    async fn round(
        &mut self,
        run: RunId,
        token: &CancellationToken,
        mut prefix: Vec<EntryBody>,
    ) -> Result<RoundEnd, StoreError> {
        let round = RoundId::new();
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
            Err(RoundError::Store(e)) => return Err(e),
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
        let (plan, request) = self.compose(round, &snapshot, safe_mode, token).await?;
        let completion = self
            .attempt(round, &snapshot, AttemptKind::Reply, &plan, request, token)
            .await?;
        for call in completion.message.tool_calls() {
            self.dispatch(round, &snapshot, &plan, call, token).await?;
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
        safe_mode: bool,
        token: &CancellationToken,
    ) -> Result<(ContextPlan, ProviderRequest), RoundError> {
        // Capture external context once. Compaction changes only the Log projection.
        let mut input = self.compose_input(snapshot, safe_mode, token).await?;
        let mut compactions = 0;
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
                    if compactions == limits::MAX_COMPACTIONS_PER_ROUND {
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
                .completion(&self.deps.profile.endpoint(kind.purpose()).plugin)
                .map_err(|failure| RoundError::Ended(RoundEnd::Failed { failure }))?
                .plugin;
            let request = crate::plan::resolve(&plan, &input.transcript, &exports, target)
                .map_err(|e| failed(code::PLAN_INVALID, e.to_string()))?;
            if matches!(kind, AttemptKind::Reply) {
                return Ok((plan, request));
            }
            self.attempt(round, snapshot, kind, &plan, request, token)
                .await?;
            compactions += 1;
            input.transcript = crate::transcript::project(&self.entries);
        }
    }

    async fn compose_input(
        &self,
        snapshot: &Snapshot,
        safe_mode: bool,
        token: &CancellationToken,
    ) -> Result<ComposeInput, RoundError> {
        let now = self.deps.clock.now();
        let latest_event = self.entries.iter().rev().find_map(|e| match &e.body {
            EntryBody::EventConsumed { event } => Some(event.clone()),
            _ => None,
        });
        let query = ContextQuery {
            session: self.session.clone(),
            latest_event,
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
        let previous_run_end = self.entries.iter().rev().find_map(|e| match &e.body {
            EntryBody::RunEnded { end, .. } => Some(end.clone()),
            _ => None,
        });
        Ok(ComposeInput {
            now,
            session: self.session.clone(),
            transcript: crate::transcript::project(&self.entries),
            previous_run_end,
            context,
            tools,
            safe_mode,
            budget: self.deps.config.budget,
        })
    }
}
