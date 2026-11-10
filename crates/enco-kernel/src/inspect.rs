use crate::{Kernel, KernelError, ProviderRequest, plan, transcript};
use enco_core::*;

/// An Attempt reconstructed exclusively from its recorded plan and Log prefix.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Inspection {
    /// Recorded Attempt identity.
    pub attempt: AttemptId,
    /// Reply or compaction.
    pub purpose: AttemptPurpose,
    /// Composer code which produced the frozen plan.
    pub composer: CodeRef,
    /// Provider code to which the recorded request was bound.
    pub provider: CodeRef,
    /// Recorded invocation parameters, excluding the credential itself.
    pub settings: ProviderSettings,
    /// Exact canonical request at this Attempt's Log position.
    pub request: ProviderRequest<Image>,
    /// Frozen plan, including each inline message's sources and all recorded omissions.
    pub plan: ContextPlan,
    /// Recorded settlement, absent while the Attempt is unsettled.
    pub result: Option<AttemptResult>,
}

impl Kernel {
    /// Inspect a specified Attempt, or the latest one, without composing or invoking a model.
    pub async fn inspect(
        &self,
        session: SessionId,
        attempt: Option<AttemptId>,
    ) -> Result<Inspection, KernelError> {
        let entries =
            crate::reach::read_back(self.deps.store.as_ref(), session, |end| reach(end, attempt))
                .await?;
        let Some((
            index,
            EntryBody::AttemptStarted {
                attempt,
                purpose,
                composer,
                provider,
                settings,
                plan: hash,
                ..
            },
        )) = started(&entries, attempt).map(|index| (index, &entries[index].body))
        else {
            return Err(KernelError::UnknownAttempt(
                session,
                attempt.map_or_else(|| "latest".into(), |id| id.to_string()),
            ));
        };
        let invalid = |reason| KernelError::InvalidPlan {
            attempt: *attempt,
            reason,
        };
        let plan = self.deps.store.plan(hash).await?;
        let target = provider
            .plugin()
            .ok_or_else(|| invalid(format!("Provider code {provider} belongs to no plugin")))?;
        let request = plan::resolve(
            &plan,
            &transcript::project(&transcript::conversation(&entries[..index])),
            target,
        )
        .map_err(|error| invalid(error.to_string()))?;
        let result = entries[index + 1..]
            .iter()
            .find_map(|entry| match &entry.body {
                EntryBody::AttemptSettled {
                    attempt: id,
                    result,
                    ..
                } if id == attempt => Some(result.clone()),
                _ => None,
            });
        Ok(Inspection {
            attempt: *attempt,
            purpose: *purpose,
            composer: composer.clone(),
            provider: provider.clone(),
            settings: settings.clone(),
            request,
            plan,
            result,
        })
    }
}

/// Where the inspected Attempt started: the specified one, or the latest.
fn started(entries: &[Entry], attempt: Option<AttemptId>) -> Option<usize> {
    entries.iter().rposition(|entry| match entry.body {
        EntryBody::AttemptStarted { attempt: id, .. } => attempt.is_none_or(|wanted| wanted == id),
        _ => false,
    })
}

/// How far back `inspect` reads: as far as projecting the conversation before the Attempt does;
/// `None` while the Attempt is not found.
fn reach(entries: &[Entry], attempt: Option<AttemptId>) -> Option<LogPos> {
    transcript::transcript_reach(&entries[..started(entries, attempt)?])
}
