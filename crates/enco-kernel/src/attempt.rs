use crate::{
    run::{RoundError, cancelled},
    session::SessionActor,
    *,
};
use enco_core::*;
use tokio_util::sync::CancellationToken;

pub(crate) enum AttemptKind {
    Reply,
    Compaction(LogPos),
}

impl AttemptKind {
    pub fn purpose(&self) -> AttemptPurpose {
        match self {
            Self::Reply => AttemptPurpose::Reply,
            Self::Compaction(_) => AttemptPurpose::Compaction,
        }
    }
}

/// The immutable request and its recorded plan, reused across retries.
pub(crate) struct PlannedAttempt {
    pub kind: AttemptKind,
    pub composer: CodeRef,
    pub plan: ContextPlan,
    pub hash: ContentHash,
    pub request: ProviderRequest,
}

impl SessionActor {
    /// Retry one combination within its own allowance, backing off between retryable failures.
    /// A failure that exhausts it, or other code taking its place, returns to the Round, which
    /// selects again from the Log; composition is pure, so the recorded plan is the same.
    pub(crate) async fn attempt(
        &mut self,
        round: RoundId,
        endpoint: &Endpoint,
        export: &Export<dyn Provider>,
        safe_mode: bool,
        planned: &PlannedAttempt,
        token: &CancellationToken,
    ) -> Result<Completion, RoundError> {
        let mut attempts = 0;
        loop {
            cancelled(token)?;
            attempts += 1;
            let failure = match self.invoke(round, endpoint, planned, export, token).await {
                Ok(completion) => return Ok(completion),
                Err(RoundError::Failed(failure)) => failure,
                Err(error) => return Err(error),
            };
            if !failure.retryable || attempts == limits::MAX_ATTEMPTS {
                return Err(RoundError::Failed(failure));
            }
            tokio::select! {
                _ = tokio::time::sleep(limits::BACKOFF[(attempts - 1) as usize]) => {},
                _ = token.cancelled() => return Err(RoundError::Cancelled),
            }
            let current = self
                .deps
                .registry
                .view()
                .exports
                .completion(endpoint, safe_mode)
                .map(|current| current.generation);
            if current != Ok(export.generation) {
                return Err(RoundError::Failed(failure));
            }
        }
    }

    async fn invoke(
        &mut self,
        round: RoundId,
        endpoint: &Endpoint,
        planned: &PlannedAttempt,
        export: &Export<dyn Provider>,
        token: &CancellationToken,
    ) -> Result<Completion, RoundError> {
        let attempt = AttemptId::new();
        self.commit(vec![EntryBody::AttemptStarted {
            round,
            attempt,
            purpose: planned.kind.purpose(),
            plan: planned.hash,
            composer: planned.composer.clone(),
            provider: export.generation,
            settings: endpoint.settings.clone(),
        }])
        .await?;
        let result = tokio::select! {
            biased;
            _ = token.cancelled() => Err(Failure {
                code: code::CANCELLED.into(), message: "model request cancelled".into(), retryable: false,
            }),
            result = export.adapter.complete(&endpoint.settings, planned.request.clone()) => result,
        };
        let result = match result {
            Ok(completion) => match self
                .settle_completion(planned, attempt, export.generation, &completion)
                .await?
            {
                Some(failure) => Err(failure),
                None => Ok(completion),
            },
            Err(failure) => {
                self.settle_failure(attempt, export.generation, &failure)
                    .await?;
                Err(failure)
            }
        };
        cancelled(token)?;
        result.map_err(RoundError::Failed)
    }

    /// Record a returned completion as it came back; its purpose decides whether it is usable,
    /// and the reason it is not, if any, is returned.
    async fn settle_completion(
        &mut self,
        planned: &PlannedAttempt,
        attempt: AttemptId,
        generation: GenerationId,
        completion: &Completion,
    ) -> Result<Option<Failure>, RoundError> {
        let result = AttemptResult::Completed {
            message: completion.message.clone(),
            usage: completion.usage.clone(),
            stop: completion.stop,
        };
        let unusable = result.failure(planned.kind.purpose());
        let mut bodies = vec![EntryBody::AttemptSettled { attempt, result }];
        if unusable.is_none()
            && let AttemptKind::Compaction(upto) = planned.kind
        {
            bodies.push(EntryBody::Compacted {
                upto,
                summary: completion.message.joined_text(),
                attempt,
            });
        }
        self.commit(bodies).await?;
        self.deps
            .registry
            .report(generation, Verdict::Ok, Some(self.session.id))
            .await?;
        Ok(unusable)
    }

    /// Record a failed call and report it to health gating.
    async fn settle_failure(
        &mut self,
        attempt: AttemptId,
        generation: GenerationId,
        failure: &Failure,
    ) -> Result<(), RoundError> {
        self.commit(vec![EntryBody::AttemptSettled {
            attempt,
            result: AttemptResult::Failed {
                failure: failure.clone(),
            },
        }])
        .await?;
        self.deps
            .registry
            .report(
                generation,
                Verdict::Failed(failure.clone()),
                Some(self.session.id),
            )
            .await?;
        Ok(())
    }
}
