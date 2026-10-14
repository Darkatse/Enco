use super::*;

pub(super) struct Prepared {
    pub generation: NewGeneration,
    pub loaded: Loaded,
}

/// Intentional changes share declaration and probe; mechanical recovery only loads.
pub(super) async fn load(
    runtime: &dyn Runtime,
    name: &str,
    artifact: &[u8],
    settings: &PluginSettings,
) -> Result<Loaded, RegistryError> {
    let loaded = runtime
        .load(artifact, settings)
        .await
        .map_err(|error| rejected(name, error))?;
    loaded.lifecycle.probe().await.map_err(|failure| {
        rejected(
            name,
            format!("probe failed: {}: {}", failure.code, failure.message),
        )
    })?;
    Ok(loaded)
}

/// The change is relative to registered intent, so an unrelated apply cannot undo a rollback.
pub(super) async fn settings(
    store: &dyn Store,
    runtime: &dyn Runtime,
    previous: &BTreeMap<String, PluginSettings>,
    next: &BTreeMap<String, PluginSettings>,
    active: &BTreeMap<String, GenerationRecord>,
    now: DateTime<Utc>,
) -> Result<BTreeMap<String, Prepared>, RegistryError> {
    let names: std::collections::BTreeSet<_> = previous.keys().chain(next.keys()).collect();
    let mut prepared = BTreeMap::new();
    for name in names {
        if previous.get(name) == next.get(name) {
            continue;
        }
        validate_name(name)?;
        let base = active
            .get(name)
            .ok_or_else(|| rejected(name, "settings require a registered active plugin"))?;
        let settings = next.get(name).cloned().unwrap_or_default();
        let bytes = store.artifact(&base.artifact).await?;
        let loaded = load(runtime, name, &bytes, &settings).await?;
        prepared.insert(
            name.clone(),
            Prepared {
                generation: NewGeneration {
                    plugin: base.plugin,
                    artifact: base.artifact,
                    settings,
                    origin: Origin::Deployed,
                    status: GenerationStatus::Trial,
                    created_at: now,
                },
                loaded,
            },
        );
    }
    Ok(prepared)
}

impl state::Plugins {
    pub fn active_records(&self) -> BTreeMap<String, GenerationRecord> {
        self.names
            .iter()
            .filter_map(|(name, plugin)| {
                let record = self.generations.get(self.active.get(plugin)?)?;
                Some((name.clone(), record.clone()))
            })
            .collect()
    }

    pub fn registered(&mut self, record: GenerationRecord, loaded: Loaded) {
        self.active.insert(record.plugin, record.id);
        self.loaded.insert(record.id, loaded);
        self.generations.insert(record.id, record);
    }
}
