use super::*;
use crate::Activation;

impl Registry {
    /// Only the current trial's attributable failures change health; late reports are harmless.
    pub async fn report(
        &self,
        generation: GenerationId,
        verdict: Verdict,
        session: Option<SessionId>,
    ) -> Result<(), RegistryError> {
        let (plugin, failure, candidates) = {
            let mut state = self.state.lock().await;
            let record = state
                .history
                .generations
                .get(&generation)
                .ok_or(StoreError::UnknownGeneration(generation))?;
            let plugin = record.plugin;
            if !current_trial(&state, generation) {
                return Ok(());
            }
            let failure = match verdict {
                Verdict::Ok => {
                    self.record_success(&mut state, generation).await?;
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
            let candidates = activation::candidates(&state, &state.history.plugins[&plugin]);
            (plugin, failure, candidates)
        };
        let recovery = activation::prepare(&self.deps, candidates).await?;
        let mut state = self.state.lock().await;
        if !current_trial(&state, generation) {
            return Ok(());
        }
        let name = state
            .running_name(plugin)
            .unwrap_or("stopped plugin")
            .to_owned();
        let mut restored = state.history.plugins[&plugin].clone();
        restored.active = recovery.target.as_ref().map(|(id, _)| *id);
        restored.rollback_target = None;
        let mut failed = recovery.failed;
        failed.push((generation, failure.clone()));
        let rollback = RolledBack {
            plugin: name,
            from: generation,
            to: restored.active.map(|reference| reference.generation),
            failure,
        };
        let event = session.map(|session| Event {
            id: EventId::new(),
            session,
            source: EventSource::Registry,
            body: EventBody::GenerationRolledBack {
                text: notice(&rollback),
                rollback,
            },
            received_at: self.deps.clock.now(),
        });
        let change = Activation {
            plugins: vec![restored],
            failed,
            event,
            ..Activation::default()
        };
        activation::commit(&self.deps, &mut state, &change).await?;
        if let Some((id, loaded)) = recovery.target {
            state.loaded.insert(id.generation, loaded);
        }
        self.publish(&mut state, None, None);
        cleanup_artifacts(&self.deps, &state).await;
        Ok(())
    }

    async fn record_success(
        &self,
        state: &mut State,
        generation: GenerationId,
    ) -> Result<(), RegistryError> {
        let successes = state.trial_successes.entry(generation).or_default();
        *successes += 1;
        if *successes == crate::TRIAL_CALLS {
            self.deps.store.promote(generation).await?;
            if let Some(record) = state.history.generations.get_mut(&generation) {
                record.status = GenerationStatus::Healthy;
            }
            self.publish(state, None, None);
        }
        Ok(())
    }
}

fn current_trial(state: &State, generation: GenerationId) -> bool {
    state
        .history
        .generations
        .get(&generation)
        .is_some_and(|record| {
            record.status == GenerationStatus::Trial
                && state.history.plugins[&record.plugin]
                    .active
                    .is_some_and(|active| active.generation == generation)
        })
}

fn notice(rollback: &RolledBack) -> String {
    let target = rollback.to.map_or_else(
        || "no generation is active".into(),
        |id| format!("now using generation {id}"),
    );
    format!(
        "[plugin rolled back] {}: generation {} failed ({}: {}); {target}",
        rollback.plugin, rollback.from, rollback.failure.code, rollback.failure.message
    )
}
