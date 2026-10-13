use crate::{
    run::{RoundError, cancelled, failed},
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
    pub request: ProviderRequest,
}

impl SessionActor {
    pub(crate) async fn attempt(
        &mut self,
        round: RoundId,
        endpoint: &Endpoint,
        safe_mode: bool,
        planned: &PlannedAttempt,
        token: &CancellationToken,
    ) -> Result<Completion, RoundError> {
        let bytes = serde_json::to_vec(&planned.plan)
            .map_err(|e| failed(code::PLAN_INVALID, e.to_string()))?;
        let plan_hash = self.deps.store.put_blob(&bytes).await?;
        let mut attempt_number = 0;
        loop {
            cancelled(token)?;
            let export = self
                .deps
                .registry
                .view()
                .exports
                .completion(&endpoint.plugin, safe_mode)
                .map_err(|failure| RoundError::Ended(RoundEnd::Failed { failure }))?;
            let attempt = AttemptId::new();
            self.commit(
                vec![EntryBody::AttemptStarted {
                    round,
                    attempt,
                    purpose: planned.kind.purpose(),
                    plan: plan_hash,
                    composer: planned.composer.clone(),
                    provider: export.generation,
                    settings: endpoint.settings.clone(),
                }],
                vec![],
            )
            .await?;
            let result = tokio::select! {
                biased;
                _ = token.cancelled() => Err(Failure {
                    code: code::CANCELLED.into(),
                    message: "model request cancelled".into(),
                    retryable: false,
                }),
                result = export.adapter.complete(&endpoint.settings, planned.request.clone()) => result,
            };
            match result {
                Ok(completion) => {
                    let unusable = self
                        .settle_completion(planned, attempt, export.generation, &completion)
                        .await?;
                    cancelled(token)?;
                    if let Some(unusable) = unusable {
                        return Err(failed(
                            code::PROVIDER_BAD_RESPONSE,
                            format!(
                                "provider returned {unusable}; stop: {}",
                                serde_json::json!(completion.stop)
                            ),
                        ));
                    }
                    return Ok(completion);
                }
                Err(failure) => {
                    let rollback = self
                        .settle_failure(attempt, export.generation, &failure)
                        .await?;
                    cancelled(token)?;
                    attempt_number += 1;
                    if attempt_number == limits::MAX_ATTEMPTS
                        || (rollback.is_none() && !failure.retryable)
                    {
                        return Err(RoundError::Ended(RoundEnd::Failed { failure }));
                    }
                    if rollback.is_some() {
                        continue;
                    }
                    let backoff = limits::BACKOFF[(attempt_number - 1) as usize];
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {},
                        _ = token.cancelled() => {},
                    }
                }
            }
        }
    }

    /// Record a returned completion as it came back; its purpose decides whether it is usable,
    /// and the reason it is not, if any, is returned.
    async fn settle_completion(
        &mut self,
        planned: &PlannedAttempt,
        attempt: AttemptId,
        generation: GenerationId,
        completion: &Completion,
    ) -> Result<Option<&'static str>, RoundError> {
        let text = completion.message.joined_text();
        let blank = text.trim().is_empty();
        let unusable = match planned.kind {
            AttemptKind::Reply if completion.message.tool_calls().next().is_some() => None,
            AttemptKind::Reply => blank.then_some("no text or tool calls"),
            AttemptKind::Compaction(_) => blank.then_some("an empty summary"),
        };
        let mut bodies = vec![EntryBody::AttemptSettled {
            attempt,
            result: AttemptResult::Completed {
                message: completion.message.clone(),
                usage: completion.usage.clone(),
                stop: completion.stop,
            },
        }];
        if unusable.is_none()
            && let AttemptKind::Compaction(upto) = planned.kind
        {
            bodies.push(EntryBody::Compacted {
                upto,
                summary: text,
                attempt,
            });
        }
        self.commit(bodies, vec![]).await?;
        self.deps
            .registry
            .report(generation, Verdict::Ok, Some(self.session.id))
            .await?;
        Ok(unusable)
    }

    /// Record a failed call and report it; the registry answers whether it rolled back.
    async fn settle_failure(
        &mut self,
        attempt: AttemptId,
        generation: GenerationId,
        failure: &Failure,
    ) -> Result<Option<RolledBack>, RoundError> {
        self.commit(
            vec![EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Failed {
                    failure: failure.clone(),
                },
            }],
            vec![],
        )
        .await?;
        Ok(self
            .deps
            .registry
            .report(
                generation,
                Verdict::Failed(failure.clone()),
                Some(self.session.id),
            )
            .await?)
    }
}
