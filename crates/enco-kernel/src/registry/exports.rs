use super::*;
use admission::Admission;
use cause::unavailable_failure;
use state::State;

/// An invocation's immutable association between code and registry identity.
pub struct Export<T: ?Sized> {
    /// Plugin owning the generation.
    pub plugin: PluginId,
    /// Exact generation used for this invocation.
    pub generation: GenerationId,
    /// Compiled adapter retained until this invocation ends.
    pub adapter: Arc<T>,
}

/// A composer generation paired with options accepted by that generation's schema.
pub struct BoundComposer {
    /// Exact code selected for this composition.
    pub export: Export<dyn Composer>,
    /// Normalized invocation options.
    pub options: ComposerOptions,
}

struct LoadedGeneration {
    plugin: PluginId,
    generation: GenerationId,
    code: CodeSource,
    loaded: Result<Loaded, Vec<Cause>>,
}

/// The generation that runs under one name in ordinary operation.
pub struct Active<'a> {
    /// Routing name.
    pub name: &'a PluginName,
    /// The running generation.
    pub generation: GenerationId,
    /// How its code entered the registry.
    pub code: CodeSource,
    /// What it declared when it loaded, or why this host cannot serve it.
    pub loaded: Result<&'a Loaded, &'a [Cause]>,
}

/// Derived routing view. Holding it does not prevent the registry from publishing a new view.
pub struct Exports {
    plugins: BTreeMap<PluginName, Routes>,
    generations: BTreeMap<GenerationId, GenerationRecord>,
    /// The factory plugin that composes in safe mode.
    composer: PluginName,
}

struct Routes {
    active: Option<LoadedGeneration>,
    factory: Option<LoadedGeneration>,
}

impl Exports {
    /// The recorded failure and current active generation for a bound invocation.
    pub(crate) fn failed(
        &self,
        generation: GenerationId,
    ) -> Option<(&Failure, Option<GenerationId>)> {
        let record = self.generations.get(&generation)?;
        let failure = record.failure.as_ref()?;
        let current = self
            .plugins
            .values()
            .filter_map(|routes| routes.active.as_ref())
            .find(|active| active.plugin == record.plugin)
            .map(|active| active.generation);
        Some((failure, current))
    }

    pub(crate) fn tools(&self) -> Vec<(ToolSpec, Export<dyn crate::Tools>)> {
        let mut tools = Vec::new();
        for name in self.plugins.keys() {
            if let Some(Ok((active, loaded))) = self.selected(name, false)
                && let Some(exported) = &loaded.tools
            {
                tools.extend(exported.specs.iter().map(|spec| {
                    (
                        spec.clone(),
                        Export {
                            plugin: active.plugin,
                            generation: active.generation,
                            adapter: exported.adapter.clone(),
                        },
                    )
                }));
            }
        }
        tools
    }

    pub(crate) fn unavailable(&self) -> Vec<(String, Failure)> {
        self.plugins
            .keys()
            .filter_map(|name| {
                let failure = match self.selected(name, false) {
                    Some(Ok(_)) => return None,
                    Some(Err(causes)) => unavailable_failure(causes),
                    None => not_running(name),
                };
                Some((name.to_string(), failure))
            })
            .collect()
    }

    pub(super) fn loaded(
        &self,
        plugin: &PluginName,
    ) -> Option<Result<(GenerationId, &Loaded), &[Cause]>> {
        self.selected(plugin, false)
            .map(|selected| selected.map(|(generation, loaded)| (generation.generation, loaded)))
    }

    pub(super) fn check_use(&self, usage: &Use) -> Result<(), Vec<Cause>> {
        admission::serve(
            usage,
            self.loaded(usage.plugin())
                .map(|selected| selected.map(|(_, loaded)| loaded)),
        )
    }

    /// Look up any recorded generation, including failed or unloaded history.
    pub fn plugin_of(&self, generation: GenerationId) -> Option<PluginId> {
        self.generations
            .get(&generation)
            .map(|record| record.plugin)
    }

    /// Resolve the active mapper; connection settings were checked during registration.
    pub fn channel(&self, plugin: &PluginName) -> Result<Export<dyn Channel>, Failure> {
        let (selected, loaded) = match self.selected(plugin, false) {
            Some(Ok(selected)) => selected,
            Some(Err(causes)) => return Err(unavailable_failure(causes)),
            None => return Err(not_running(plugin)),
        };
        let adapter = loaded.channel.clone().ok_or_else(|| Failure {
            code: code::PLUGIN_UNAVAILABLE.into(),
            message: format!("plugin {plugin}: requires channel"),
            retryable: false,
        })?;
        Ok(Export {
            plugin: selected.plugin,
            generation: selected.generation,
            adapter,
        })
    }

    /// Resolve a completion use, using the Round's fixed mode.
    pub fn completion(
        &self,
        endpoint: &Endpoint,
        safe_mode: bool,
    ) -> Result<Export<dyn Provider>, Failure> {
        self.export(endpoint, safe_mode, "completion", |loaded| {
            loaded.completion.clone()
        })
    }

    /// Resolve embedding at the start of one synchronization call.
    pub fn embedding(&self, endpoint: &Endpoint) -> Result<Export<dyn Embedding>, Failure> {
        self.export(endpoint, false, "embedding", |loaded| {
            loaded.embedding.clone()
        })
    }

    /// Resolve decision at the start of one policy evaluation.
    pub fn decision(&self, endpoint: &Endpoint) -> Result<Export<dyn Decision>, Failure> {
        self.export(endpoint, false, "decision", |loaded| {
            loaded.decision.clone()
        })
    }

    /// Select the current composer and normalize its profile options in one binding step.
    ///
    /// Safe mode always uses the factory generation of the host's composer with empty options.
    pub fn composer(
        &self,
        settings: &ComposerSettings,
        safe_mode: bool,
    ) -> Result<BoundComposer, Failure> {
        let empty = ComposerOptions::new();
        let (name, options) = if safe_mode {
            (&self.composer, &empty)
        } else {
            (&settings.plugin, &settings.options)
        };
        let (selected, loaded) = match self.selected(name, safe_mode) {
            Some(Ok(selected)) => selected,
            Some(Err(causes)) => return Err(unavailable_failure(causes)),
            None => return Err(not_running(name)),
        };
        let (adapter, options) =
            wiring::bind_composer(loaded, options).map_err(|mut failure| {
                failure.message = format!(
                    "composer {name}, generation {}: {}",
                    selected.generation, failure.message
                );
                failure
            })?;
        Ok(BoundComposer {
            export: Export {
                plugin: selected.plugin,
                generation: selected.generation,
                adapter,
            },
            options,
        })
    }

    /// Absent when no generation runs under the name.
    fn selected(
        &self,
        plugin: &PluginName,
        safe_mode: bool,
    ) -> Option<Result<(&LoadedGeneration, &Loaded), &[Cause]>> {
        let routes = self.plugins.get(plugin)?;
        // Safe mode replaces only what the host ships; a plugin without factory code keeps its own.
        let selected = if safe_mode {
            routes.factory.as_ref().or(routes.active.as_ref())
        } else {
            routes.active.as_ref()
        }?;
        Some(
            selected
                .loaded
                .as_ref()
                .map(|loaded| (selected, loaded))
                .map_err(Vec::as_slice),
        )
    }

    fn export<T: ?Sized>(
        &self,
        endpoint: &Endpoint,
        safe_mode: bool,
        interface: &str,
        adapter: impl FnOnce(&Loaded) -> Option<Arc<T>>,
    ) -> Result<Export<T>, Failure> {
        let (selected, loaded) = match self.selected(&endpoint.plugin, safe_mode) {
            Some(Ok(selected)) => selected,
            Some(Err(causes)) => return Err(unavailable_failure(causes)),
            None => return Err(not_running(&endpoint.plugin)),
        };
        let adapter = endpoint.export(loaded, adapter(loaded), interface)?;
        Ok(Export {
            plugin: selected.plugin,
            generation: selected.generation,
            adapter,
        })
    }

    /// The active generation behind every name, in name order.
    pub(super) fn active_generations(&self) -> impl Iterator<Item = GenerationId> + '_ {
        self.active().map(|active| active.generation)
    }

    /// The generation that runs under each name in ordinary operation, in name order.
    pub fn active(&self) -> impl Iterator<Item = Active<'_>> {
        self.plugins.iter().filter_map(|(name, routes)| {
            let active = routes.active.as_ref()?;
            Some(Active {
                name,
                generation: active.generation,
                code: active.code,
                loaded: active.loaded.as_ref().map_err(Vec::as_slice),
            })
        })
    }
}

fn not_running(plugin: &PluginName) -> Failure {
    Failure {
        code: code::PLUGIN_UNAVAILABLE.into(),
        message: format!("plugin {plugin}: no generation runs"),
        retryable: false,
    }
}

impl State {
    /// Bind one already computed admission result to the identities assigned by the commit.
    pub fn exports(
        &self,
        admission: Admission,
        factory: &[FactoryPlugin],
        composer: &PluginName,
    ) -> Exports {
        let mut plugins = BTreeMap::new();
        let mut names = self.running_names();
        names.extend(
            factory
                .iter()
                .map(|plugin| (plugin.name.clone(), plugin.id)),
        );
        for (name, plugin) in names {
            plugins.insert(
                name.clone(),
                Routes {
                    active: self
                        .history
                        .plugins
                        .get(&plugin)
                        .and_then(|record| record.active)
                        .zip(admission.result(&name))
                        .map(|(generation, loaded)| LoadedGeneration {
                            plugin,
                            generation: generation.generation,
                            code: self.provenance(generation.generation).source,
                            loaded,
                        }),
                    factory: self.factory.get(&plugin).and_then(|generation| {
                        self.loaded.get(generation).map(|loaded| LoadedGeneration {
                            plugin,
                            generation: *generation,
                            code: CodeSource::Factory,
                            loaded: Ok(loaded.clone()),
                        })
                    }),
                },
            );
        }
        Exports {
            plugins,
            generations: self.history.generations.clone(),
            composer: composer.clone(),
        }
    }
}
