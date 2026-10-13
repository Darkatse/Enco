use super::*;
use state::Plugins;

/// An invocation's immutable association between code and registry identity.
pub struct Export<T: ?Sized> {
    /// Plugin owning the generation.
    pub plugin: PluginId,
    /// Exact generation used for this invocation.
    pub generation: GenerationId,
    /// Compiled adapter retained until this invocation ends.
    pub adapter: Arc<T>,
}

struct LoadedGeneration {
    plugin: PluginId,
    generation: GenerationId,
    loaded: Loaded,
}

/// Derived routing view. Holding it does not prevent the registry from publishing a new view.
pub struct Exports {
    plugins: BTreeMap<String, Routes>,
    generations: BTreeMap<GenerationId, PluginId>,
}

struct Routes {
    active: Option<LoadedGeneration>,
    factory: Option<LoadedGeneration>,
}

impl Exports {
    /// Look up any recorded generation, including failed or unloaded history.
    pub fn plugin_of(&self, generation: GenerationId) -> Option<PluginId> {
        self.generations.get(&generation).copied()
    }

    fn routes(&self, plugin: &str) -> Result<&Routes, Failure> {
        self.plugins
            .get(plugin)
            .ok_or_else(|| unavailable(plugin, "name is not registered"))
    }

    /// Resolve completion at the start of an Attempt using the Round's fixed mode.
    pub fn completion(
        &self,
        plugin: &str,
        safe_mode: bool,
    ) -> Result<Export<dyn Provider>, Failure> {
        let routes = self.routes(plugin)?;
        let selected = if safe_mode {
            routes.factory.as_ref().or(routes.active.as_ref())
        } else {
            routes.active.as_ref()
        }
        .ok_or_else(|| unavailable(plugin, "no active generation"))?;
        selected.export(plugin, "completion", |loaded| loaded.completion.clone())
    }

    /// Resolve embedding at the start of one synchronization call.
    pub fn embedding(&self, plugin: &str) -> Result<Export<dyn Embedding>, Failure> {
        self.active(plugin)?
            .export(plugin, "embedding", |loaded| loaded.embedding.clone())
    }

    /// Resolve decision at the start of one policy evaluation.
    pub fn decision(&self, plugin: &str) -> Result<Export<dyn Decision>, Failure> {
        self.active(plugin)?
            .export(plugin, "decision", |loaded| loaded.decision.clone())
    }

    /// The active generation behind a name, which a commit expects to be unchanged.
    pub(super) fn active_generation(&self, plugin: &str) -> Option<GenerationId> {
        self.plugins
            .get(plugin)?
            .active
            .as_ref()
            .map(|loaded| loaded.generation)
    }

    fn active(&self, plugin: &str) -> Result<&LoadedGeneration, Failure> {
        self.routes(plugin)?
            .active
            .as_ref()
            .ok_or_else(|| unavailable(plugin, "no active generation"))
    }
}

impl LoadedGeneration {
    /// Bind one interface of this generation to the invocation that will record it.
    fn export<T: ?Sized>(
        &self,
        plugin: &str,
        interface: &str,
        adapter: impl FnOnce(&Loaded) -> Option<Arc<T>>,
    ) -> Result<Export<T>, Failure> {
        let adapter = adapter(&self.loaded).ok_or_else(|| {
            unavailable(
                plugin,
                &format!("generation {} does not export {interface}", self.generation),
            )
        })?;
        Ok(Export {
            plugin: self.plugin,
            generation: self.generation,
            adapter,
        })
    }
}

fn unavailable(plugin: &str, reason: &str) -> Failure {
    Failure {
        code: code::PLUGIN_UNAVAILABLE.into(),
        message: format!("plugin {plugin}: {reason}"),
        retryable: false,
    }
}

impl Plugins {
    /// Derive the routes a published view carries from the committer's state.
    pub fn exports(&self) -> Exports {
        Exports {
            generations: self
                .generations
                .iter()
                .map(|(id, record)| (*id, record.plugin))
                .collect(),
            plugins: self
                .names
                .iter()
                .map(|(name, plugin)| {
                    let export = |generation: GenerationId| {
                        self.loaded.get(&generation).map(|loaded| LoadedGeneration {
                            plugin: *plugin,
                            generation,
                            loaded: loaded.clone(),
                        })
                    };
                    (
                        name.clone(),
                        Routes {
                            active: self.active.get(plugin).copied().and_then(export),
                            factory: self.factory.get(plugin).copied().and_then(export),
                        },
                    )
                })
                .collect(),
        }
    }
}
