use super::*;
use admission::Admission;
use cause::unavailable_failure;
use state::State;

/// An invocation's immutable association between code and registry identity.
pub struct Export<T: ?Sized> {
    /// Exact code used for this invocation, a generation or factory code, and its plugin.
    pub code: CodeRef,
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

struct LoadedCode {
    code: CodeRef,
    source: CodeSource,
    loaded: Result<Loaded, Vec<Cause>>,
}

/// The code that runs under one name in ordinary operation.
pub struct Active<'a> {
    /// Routing name.
    pub name: &'a PluginName,
    /// The running code.
    pub code: &'a CodeRef,
    /// How its code entered the registry.
    pub source: CodeSource,
    /// What it declared when it loaded, or why this host cannot serve it.
    pub loaded: Result<&'a Loaded, &'a [Cause]>,
}

/// Derived routing view. Holding it does not prevent the registry from publishing a new view.
pub struct Exports {
    plugins: BTreeMap<PluginName, Routes>,
    /// Each generation's standing on this node, for calls bound before a rollback.
    health: BTreeMap<GenerationId, Health>,
    /// The factory plugin that composes in safe mode.
    composer: PluginName,
}

struct Routes {
    active: Option<LoadedCode>,
    factory: Option<LoadedCode>,
}

/// Code that has failed, and what its plugin runs now.
pub(crate) struct Failed<'a> {
    /// The generation that failed.
    pub from: GenerationId,
    /// The fault that marked the generation failed.
    pub failure: &'a Failure,
    /// What its plugin runs instead, absent when nothing can run.
    pub current: Option<PluginCode>,
}

impl Exports {
    /// The recorded failure of code bound before it failed, with what its plugin runs now.
    ///
    /// Whether it failed is the health record's to say alone; nothing running in its place leaves
    /// `current` empty.
    pub(crate) fn failed(&self, code: &CodeRef) -> Option<Failed<'_>> {
        let generation = code.generation()?;
        let failure = self.health.get(&generation)?.failure()?;
        let current = self
            .plugins
            .values()
            .filter_map(|routes| routes.active.as_ref())
            .find(|active| active.code.plugin == generation.plugin)
            .map(|active| active.code.code);
        Some(Failed {
            from: generation,
            failure,
            current,
        })
    }

    /// Each running plugin's tools, with the name the plugin runs under.
    pub(crate) fn tools(&self) -> Vec<(&PluginName, ToolSpec, Export<dyn crate::Tools>)> {
        let mut tools = Vec::new();
        for name in self.plugins.keys() {
            if let Some(Ok((active, loaded))) = self.selected(name, false)
                && let Some(exported) = &loaded.tools
            {
                tools.extend(exported.specs.iter().map(|spec| {
                    (
                        name,
                        spec.clone(),
                        Export {
                            code: active.code,
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
    ) -> Option<Result<(CodeRef, &Loaded), &[Cause]>> {
        self.selected(plugin, false)
            .map(|selected| selected.map(|(selected, loaded)| (selected.code, loaded)))
    }

    pub(super) fn check_use(&self, usage: &Use) -> Result<(), Vec<Cause>> {
        admission::serve(
            usage,
            self.loaded(usage.plugin())
                .map(|selected| selected.map(|(_, loaded)| loaded)),
        )
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
            code: selected.code,
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
    /// Safe mode always uses the host's factory composer code with empty options.
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
                failure.message =
                    format!("composer {name}, {}: {}", selected.code, failure.message);
                failure
            })?;
        Ok(BoundComposer {
            export: Export {
                code: selected.code,
                adapter,
            },
            options,
        })
    }

    /// Absent when nothing runs under the name.
    fn selected(
        &self,
        plugin: &PluginName,
        safe_mode: bool,
    ) -> Option<Result<(&LoadedCode, &Loaded), &[Cause]>> {
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
            code: selected.code,
            adapter,
        })
    }

    /// The code every running plugin uses, by identity.
    pub(super) fn active_code(&self) -> BTreeMap<PluginId, PluginCode> {
        self.plugins
            .values()
            .filter_map(|routes| routes.active.as_ref())
            .map(|active| (active.code.plugin, active.code.code))
            .collect()
    }

    /// The code that runs under each name in ordinary operation, in name order.
    pub fn active(&self) -> impl Iterator<Item = Active<'_>> {
        self.plugins.iter().filter_map(|(name, routes)| {
            let active = routes.active.as_ref()?;
            Some(Active {
                name,
                code: &active.code,
                source: active.source,
                loaded: active.loaded.as_ref().map_err(Vec::as_slice),
            })
        })
    }
}

fn not_running(plugin: &PluginName) -> Failure {
    Failure {
        code: code::PLUGIN_UNAVAILABLE.into(),
        message: format!("plugin {plugin}: nothing runs"),
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
    ) -> Result<Exports, RegistryError> {
        let mut plugins = BTreeMap::new();
        let mut names = self.running_names();
        names.extend(
            factory
                .iter()
                .map(|plugin| (plugin.name.clone(), plugin.id)),
        );
        for (name, plugin) in names {
            let active = match self.active(plugin).zip(admission.result(&name)) {
                Some((code, loaded)) => Some(LoadedCode {
                    source: self.provenance(code)?.source,
                    code,
                    loaded,
                }),
                None => None,
            };
            let factory = self.factory_code(plugin).and_then(|code| {
                self.loaded.get(&code).map(|loaded| LoadedCode {
                    code,
                    source: CodeSource::Factory,
                    loaded: Ok(loaded.clone()),
                })
            });
            plugins.insert(name.clone(), Routes { active, factory });
        }
        Ok(Exports {
            plugins,
            health: self.node.health.clone(),
            composer: composer.clone(),
        })
    }
}
