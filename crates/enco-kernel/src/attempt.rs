use crate::{
    run::{RoundError, cancelled, failed},
    session::SessionActor,
    snapshot::Snapshot,
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

impl SessionActor {
    pub(crate) async fn attempt(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        kind: AttemptKind,
        plan: &ContextPlan,
        request: ProviderRequest,
        token: &CancellationToken,
    ) -> Result<Completion, RoundError> {
        let bytes =
            serde_json::to_vec(plan).map_err(|e| failed(code::PLAN_INVALID, e.to_string()))?;
        let plan_hash = self.deps.store.put_blob(&bytes).await?;
        let mut attempt_number = 0;
        loop {
            cancelled(token)?;
            let attempt = AttemptId::new();
            self.commit(
                vec![EntryBody::AttemptStarted {
                    round,
                    attempt,
                    purpose: kind.purpose(),
                    plan: plan_hash,
                    composer: snapshot.composer.code(),
                    provider: snapshot.provider.code(),
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
                result = snapshot.provider.complete(request.clone()) => result,
            };
            match result {
                Ok(completion) => {
                    let mut bodies = vec![EntryBody::AttemptSettled {
                        attempt,
                        result: AttemptResult::Completed {
                            message: completion.message.clone(),
                            usage: completion.usage.clone(),
                            stop: completion.stop,
                        },
                    }];
                    if let AttemptKind::Compaction(upto) = kind {
                        let summary = completion.message.joined_text();
                        if summary.trim().is_empty() {
                            self.commit(bodies, vec![]).await?;
                            cancelled(token)?;
                            return Err(failed(
                                code::COMPOSE_FAILED,
                                "provider returned an empty summary",
                            ));
                        }
                        bodies.push(EntryBody::Compacted {
                            upto,
                            summary,
                            attempt,
                        });
                    }
                    self.commit(bodies, vec![]).await?;
                    return Ok(completion);
                }
                Err(failure) => {
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
                    cancelled(token)?;
                    if !failure.retryable || attempt_number + 1 == limits::MAX_ATTEMPTS {
                        return Err(RoundError::Ended(RoundEnd::Failed { failure }));
                    }
                    let backoff = limits::BACKOFF[attempt_number as usize];
                    attempt_number += 1;
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {},
                        _ = token.cancelled() => {},
                    }
                }
            }
        }
    }
}
