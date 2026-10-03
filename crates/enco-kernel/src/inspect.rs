use crate::{Kernel, KernelError, ProviderRequest, plan, transcript};
use enco_core::*;

/// An Attempt reconstructed exclusively from its recorded plan and Log prefix.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Inspection {
    /// Recorded Attempt identity.
    pub attempt: AttemptId,
    /// Reply or compaction.
    pub purpose: AttemptPurpose,
    /// Code which produced the frozen plan.
    pub composer: CodeRef,
    /// Code to which the recorded request was bound.
    pub provider: CodeRef,
    /// Recorded invocation parameters, excluding the credential itself.
    pub settings: ProviderSettings,
    /// Exact canonical request at this Attempt's Log position.
    pub request: ProviderRequest,
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
        let entries = self.deps.store.log(session, None).await?;
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
        )) = entries.iter().enumerate().rev()
            .map(|(index, entry)| (index, &entry.body))
            .find(|(_, body)| matches!(body, EntryBody::AttemptStarted { attempt: id, .. } if attempt.is_none_or(|wanted| wanted == *id)))
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
        let bytes = self.deps.store.get_blob(hash).await?;
        let plan: ContextPlan =
            serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
        let exports = self.deps.registry.exports();
        let target = match provider {
            CodeRef::Generation { id } => exports.plugin_of(*id),
            CodeRef::Native { .. } => None,
        }
        .ok_or_else(|| invalid("recorded provider has no registered plugin identity".into()))?;
        let request = plan::resolve(
            &plan,
            &transcript::project(&entries[..index]),
            &exports,
            target,
        )
        .map_err(|error| invalid(error.to_string()))?;
        let result = entries[index + 1..]
            .iter()
            .find_map(|entry| match &entry.body {
                EntryBody::AttemptSettled {
                    attempt: id,
                    result,
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
