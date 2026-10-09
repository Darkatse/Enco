use super::*;

impl Registry {
    /// Observe one settled call. Only the current trial generation can change health.
    /// A Session notice, when requested, commits together with automatic recovery.
    pub async fn report(
        &self,
        generation: GenerationId,
        verdict: Verdict,
        session: Option<SessionId>,
    ) -> Result<Option<RolledBack>, RegistryError> {
        let (name, plugin, candidates, failure) = {
            let mut state = self.state.lock().await;
            let record = state
                .generations
                .get(&generation)
                .ok_or(StoreError::UnknownGeneration(generation))?;
            let plugin = record.plugin;
            if record.status != GenerationStatus::Trial
                || state.active.get(&plugin) != Some(&generation)
            {
                return Ok(None);
            }
            let failure = match verdict {
                Verdict::Ok => {
                    let successes =
                        state.trial_successes.get(&generation).copied().unwrap_or(0) + 1;
                    if successes == crate::TRIAL_CALLS {
                        self.store.promote(generation).await?;
                        // The record was checked above and the commit lock is still held.
                        if let Some(record) = state.generations.get_mut(&generation) {
                            record.status = GenerationStatus::Healthy;
                        }
                        self.publish(&mut state);
                    } else {
                        state.trial_successes.insert(generation, successes);
                    }
                    return Ok(None);
                }
                Verdict::Failed(failure)
                    if matches!(
                        failure.code.as_str(),
                        code::PLUGIN_TRAP | code::PLUGIN_CONTRACT
                    ) =>
                {
                    failure
                }
                Verdict::Failed(_) => return Ok(None),
            };
            let name = state.name_of(plugin)?.to_owned();
            (name, plugin, state.candidates(plugin, generation), failure)
        };
        match self
            .activate(
                &name,
                plugin,
                generation,
                candidates,
                ActivationReason::TrialFailure { failure, session },
            )
            .await
        {
            Ok(activation) => Ok(activation.rollback),
            // Deployment or promotion won while the fallback was being loaded.
            Err(RegistryError::Conflict(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
