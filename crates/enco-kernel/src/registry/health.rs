use super::*;

impl Registry {
    /// Observe one settled call. Only the current trial generation can change health.
    /// A Session notice, when requested, commits together with automatic recovery; callers
    /// see a rollback as a changed view, not as a return value.
    pub async fn report(
        &self,
        generation: GenerationId,
        verdict: Verdict,
        session: Option<SessionId>,
    ) -> Result<(), RegistryError> {
        let (name, plugin, candidates, failure) = {
            let mut state = self.state.lock().await;
            let record = state
                .plugins
                .generations
                .get(&generation)
                .ok_or(StoreError::UnknownGeneration(generation))?;
            let plugin = record.plugin;
            if record.status != GenerationStatus::Trial
                || state.plugins.active.get(&plugin) != Some(&generation)
            {
                return Ok(());
            }
            let failure = match verdict {
                Verdict::Ok => {
                    let successes = state
                        .plugins
                        .trial_successes
                        .get(&generation)
                        .copied()
                        .unwrap_or(0)
                        + 1;
                    if successes == crate::TRIAL_CALLS {
                        self.store.promote(generation).await?;
                        // The record was checked above and the commit lock is still held.
                        if let Some(record) = state.plugins.generations.get_mut(&generation) {
                            record.status = GenerationStatus::Healthy;
                        }
                        self.publish(&mut state, None);
                    } else {
                        state.plugins.trial_successes.insert(generation, successes);
                    }
                    return Ok(());
                }
                Verdict::Failed(failure)
                    if matches!(
                        failure.code.as_str(),
                        code::PLUGIN_TRAP | code::PLUGIN_CONTRACT
                    ) =>
                {
                    failure
                }
                Verdict::Failed(_) => return Ok(()),
            };
            let name = state.plugins.name_of(plugin)?.to_owned();
            (
                name,
                plugin,
                state.plugins.candidates(plugin, generation),
                failure,
            )
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
            // Deployment or promotion won while the fallback was being loaded.
            Ok(_) | Err(RegistryError::Conflict(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
