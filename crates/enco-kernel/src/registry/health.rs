use super::*;
use crate::ActivationWrite;

impl Registry {
    /// Only the current trial's attributable failures change health; late reports, and reports
    /// about factory code, which is never on trial, are harmless.
    pub async fn report(
        &self,
        code: &CodeRef,
        verdict: Verdict,
        session: Option<SessionId>,
    ) -> Result<(), RegistryError> {
        let Some(generation) = code.generation() else {
            return Ok(());
        };
        let (failure, candidates) = {
            let mut state = self.state.lock().await;
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
            let candidates =
                activation::candidates(&state, &state.node.plugins[&generation.plugin])?;
            (failure, candidates)
        };
        let recovery = activation::prepare(&self.deps, candidates).await?;
        let mut state = self.state.lock().await;
        if !current_trial(&state, generation) {
            return Ok(());
        }
        let plugin = generation.plugin;
        let name = state.running_name(plugin).cloned().ok_or_else(|| {
            RegistryError::Intent(format!("plugin {plugin} on trial has no running name"))
        })?;
        let mut restored = state.node.plugins[&plugin].clone();
        restored.active = recovery.target.as_ref().map(|(code, _)| code.clone());
        restored.rollback_target = None;
        let mut failed = recovery.failed;
        failed.push((generation, failure.clone()));
        let rollback = RolledBack {
            plugin: name,
            from: generation,
            to: restored
                .active
                .as_ref()
                .map(|reference| reference.code.clone()),
            failure,
        };
        let event = session.map(|session| Event {
            id: EventId::new(),
            session,
            source: EventSource::Registry,
            body: EventBody::PluginRolledBack {
                text: notice(&rollback),
                rollback,
            },
            received_at: self.deps.clock.now(),
        });
        let change = ActivationWrite {
            plugins: vec![restored],
            failed,
            event,
            ..ActivationWrite::default()
        };
        activation::commit(&self.deps, &mut state, &change).await?;
        if let Some((reference, loaded)) = recovery.target {
            state.loaded.insert(reference.code, loaded);
        }
        self.publish(&mut state, None, None)?;
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
            state.node.health.insert(generation, Health::Healthy);
            self.publish(state, None, None)?;
        }
        Ok(())
    }
}

/// Whether the generation is on trial on this node and is its plugin's current code.
fn current_trial(state: &State, generation: GenerationId) -> bool {
    state.node.health.get(&generation) == Some(&Health::Trial)
        && state
            .node
            .plugins
            .get(&generation.plugin)
            .and_then(|plugin| plugin.active.as_ref())
            .is_some_and(|active| active.code.generation() == Some(generation))
}

/// The notice the model reads: what failed, what runs now, and that nothing registered changed.
fn notice(rollback: &RolledBack) -> String {
    let failed = rollback.from.registration;
    let cause = format!(
        "{} failed on trial with registration {failed} ({}: {})",
        rollback.plugin, rollback.failure.code, rollback.failure.message
    );
    let outcome = match &rollback.to {
        Some(code) => format!("{cause} and was rolled back to {code}."),
        None => format!("{cause}; nothing earlier can run, so it is stopped."),
    };
    format!(
        "[plugin rolled back] {outcome} Registration {failed} and the files in the home are unchanged."
    )
}
