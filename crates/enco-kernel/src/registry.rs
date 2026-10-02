//! The single committer of plugin identities, activation history and published exports.
mod activation;
mod state;

use crate::{Clock, Embedding, Loaded, NewGeneration, Provider, Runtime, Store, StoreError};
use arc_swap::ArcSwap;
use enco_core::*;
use state::State;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;

/// Durable registry dependencies assembled by the application.
pub struct RegistryDeps {
    /// Sole persistence port, including plugins.lock and immutable artifacts.
    pub store: Arc<dyn Store>,
    /// Component compilation and export discovery.
    pub runtime: Arc<dyn Runtime>,
    /// Host-reserved identities and embedded bytes.
    pub factory: Vec<FactoryPlugin>,
    /// Interfaces required by configured callers.
    pub wiring: Vec<Use>,
    /// Observation time, never activation ordering.
    pub clock: Arc<dyn Clock>,
}

/// A plugin supplied with the host binary.
pub struct FactoryPlugin {
    /// Reserved name in plugins.lock.
    pub name: String,
    /// Stable host-assigned identity, independent of component self-description.
    pub id: PluginId,
    /// Embedded component bytes.
    pub artifact: Vec<u8>,
}

/// One configured caller's required export.
#[derive(Clone)]
pub struct Use {
    /// Human-readable caller, such as "profile default.reply" or "embedding".
    pub user: String,
    /// Registered plugin name.
    pub plugin: String,
    /// Required interface.
    pub interface: Interface,
}

/// Interfaces currently consumed by the host and kernel.
#[derive(Clone, Copy)]
pub enum Interface {
    /// Session completion.
    Completion,
    /// Memory embedding.
    Embedding,
}

/// Routing is published only after the durable mutation commits.
pub struct Registry {
    store: Arc<dyn Store>,
    runtime: Arc<dyn Runtime>,
    clock: Arc<dyn Clock>,
    wiring: Vec<Use>,
    state: Mutex<State>,
    exports: ArcSwap<Exports>,
}

/// An invocation's immutable association between code and registry identity.
pub struct Export<T: ?Sized> {
    /// Plugin owning the activation.
    pub plugin: PluginId,
    /// Exact activation used for this invocation.
    pub generation: GenerationId,
    /// Compiled adapter retained until this invocation ends.
    pub adapter: Arc<T>,
}

struct Active {
    plugin: PluginId,
    generation: GenerationId,
    loaded: Loaded,
}

/// Derived routing view. Holding it does not prevent the registry from publishing a new view.
#[derive(Default)]
pub struct Exports {
    active: BTreeMap<String, Option<Active>>,
}

impl Exports {
    fn active(&self, plugin: &str) -> Result<&Active, Failure> {
        self.active
            .get(plugin)
            .ok_or_else(|| unavailable(plugin, "name is not registered"))?
            .as_ref()
            .ok_or_else(|| unavailable(plugin, "no active generation"))
    }

    /// Resolve completion at the start of an Attempt.
    pub fn completion(&self, plugin: &str) -> Result<Export<dyn Provider>, Failure> {
        let active = self.active(plugin)?;
        let adapter =
            active.loaded.completion.clone().ok_or_else(|| {
                unavailable(plugin, "active generation does not export completion")
            })?;
        Ok(Export {
            plugin: active.plugin,
            generation: active.generation,
            adapter,
        })
    }

    /// Resolve embedding at the start of one synchronization call.
    pub fn embedding(&self, plugin: &str) -> Result<Export<dyn Embedding>, Failure> {
        let active = self.active(plugin)?;
        let adapter =
            active.loaded.embedding.clone().ok_or_else(|| {
                unavailable(plugin, "active generation does not export embedding")
            })?;
        Ok(Export {
            plugin: active.plugin,
            generation: active.generation,
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

/// Result of accepting a component as a new activation.
#[derive(serde::Serialize)]
pub struct Deployed {
    /// Newly committed activation.
    pub generation: GenerationRecord,
    /// Configured callers affected by the change.
    pub users: Vec<String>,
}

/// Inspectable identity, history and routing for one registered name.
#[derive(serde::Serialize)]
pub struct PluginStatus {
    /// Human-facing name.
    pub name: String,
    /// Stable identity from plugins.lock.
    pub id: PluginId,
    /// Current activation, if available.
    pub active: Option<GenerationRecord>,
    /// Description of the active component, if loaded.
    pub summary: Option<String>,
    /// Nearest earlier healthy activation.
    pub rollback_target: Option<GenerationId>,
    /// Configured users of this plugin.
    pub users: Vec<String>,
    /// Complete history in registry sequence order.
    pub generations: Vec<GenerationRecord>,
}

/// An explicit registry operation could not complete.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// A durable operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A reserved identity in plugins.lock differs from the host's identity.
    #[error("plugins.lock records a different identity for factory plugin {0}")]
    FactoryIdentity(String),
    /// No registered plugin matches this name or identity.
    #[error("unknown plugin {0}")]
    UnknownPlugin(String),
    /// The component, name or its exports cannot satisfy the operation.
    #[error("plugin {name} rejected: {reason}")]
    Rejected {
        /// Name supplied by the caller.
        name: String,
        /// Loading or admission failure.
        reason: String,
    },
    /// No earlier usable activation exists.
    #[error("plugin {0} has no generation to roll back to")]
    NoRollbackTarget(String),
    /// The expected activation or target health changed while the target was prepared.
    #[error("plugin {0}: the registry changed while preparing; retry")]
    Conflict(String),
}

impl Registry {
    /// Restore persisted routing, register embedded artifacts and validate configured uses.
    pub async fn open(deps: RegistryDeps) -> Result<Arc<Self>, RegistryError> {
        let state = State::read(&deps).await?;
        let registry = Arc::new(Self {
            store: deps.store,
            runtime: deps.runtime,
            clock: deps.clock,
            wiring: deps.wiring,
            state: Mutex::new(state),
            exports: ArcSwap::from_pointee(Exports::default()),
        });
        registry.restore().await?;
        Ok(registry)
    }

    /// Take an immutable view for one invocation.
    pub fn exports(&self) -> Arc<Exports> {
        self.exports.load_full()
    }

    /// Load outside the commit lock, then admit and activate against the latest state.
    pub async fn deploy(&self, name: &str, artifact: Vec<u8>) -> Result<Deployed, RegistryError> {
        validate_name(name)?;
        let hash = self.store.put_artifact(&artifact).await?;
        let config = serde_json::json!({});
        let loaded = self
            .runtime
            .load(&artifact, &config)
            .await
            .map_err(|error| RegistryError::Rejected {
                name: name.into(),
                reason: error.to_string(),
            })?;
        let mut state = self.state.lock().await;
        self.admit(name, &loaded)?;
        let plugin = match state.names.get(name) {
            Some(id) => *id,
            None => {
                let id = PluginId::new();
                self.store.register_plugin(name, id).await?;
                state.names.insert(name.into(), id);
                id
            }
        };
        let generation = NewGeneration {
            plugin,
            artifact: hash,
            config,
            origin: Origin::Deployed,
            status: GenerationStatus::Healthy,
            created_at: self.clock.now().to_utc(),
        };
        let id = self.store.insert_generation(&generation, true).await?;
        let record = generation.numbered(id);
        state.generations.insert(id, record.clone());
        state.active.insert(plugin, Some(id));
        state.loaded.insert(id, loaded);
        self.publish(&mut state);
        Ok(Deployed {
            generation: record,
            users: self.users(name),
        })
    }

    /// Roll back relative to the generation observed when choosing the target.
    pub async fn rollback(&self, name: &str) -> Result<GenerationRecord, RegistryError> {
        let (plugin, expected, candidates) = {
            let state = self.state.lock().await;
            let plugin = *state
                .names
                .get(name)
                .ok_or_else(|| RegistryError::UnknownPlugin(name.into()))?;
            let expected = state
                .active
                .get(&plugin)
                .copied()
                .flatten()
                .ok_or_else(|| RegistryError::NoRollbackTarget(name.into()))?;
            (plugin, expected, state.candidates(plugin, expected))
        };
        if candidates.is_empty() {
            return Err(RegistryError::NoRollbackTarget(name.into()));
        }
        self.activate(name, plugin, expected, candidates, None)
            .await?
            .ok_or_else(|| RegistryError::NoRollbackTarget(name.into()))
    }

    /// Read the registry's single owned state without maintaining a second status cache.
    pub async fn status(&self) -> Vec<PluginStatus> {
        let state = self.state.lock().await;
        state
            .names
            .iter()
            .map(|(name, id)| {
                let active = state
                    .active
                    .get(id)
                    .copied()
                    .flatten()
                    .and_then(|id| state.generations.get(&id));
                PluginStatus {
                    name: name.clone(),
                    id: *id,
                    summary: active
                        .and_then(|record| state.loaded.get(&record.id))
                        .map(|loaded| loaded.summary.clone()),
                    rollback_target: active.and_then(|record| {
                        state
                            .candidates(*id, record.id)
                            .first()
                            .map(|record| record.id)
                    }),
                    active: active.cloned(),
                    users: self.users(name),
                    generations: state
                        .generations
                        .values()
                        .filter(|record| record.plugin == *id)
                        .cloned()
                        .collect(),
                }
            })
            .collect()
    }

    fn users(&self, name: &str) -> Vec<String> {
        self.wiring
            .iter()
            .filter(|usage| usage.plugin == name)
            .map(|usage| usage.user.clone())
            .collect()
    }

    fn admit(&self, name: &str, loaded: &Loaded) -> Result<(), RegistryError> {
        let missing: Vec<_> = self
            .wiring
            .iter()
            .filter(|usage| usage.plugin == name)
            .filter_map(|usage| {
                let interface = match usage.interface {
                    Interface::Completion if loaded.completion.is_none() => "completion",
                    Interface::Embedding if loaded.embedding.is_none() => "embedding",
                    _ => return None,
                };
                Some(format!("{} requires {interface}", usage.user))
            })
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(RegistryError::Rejected {
                name: name.into(),
                reason: format!("missing exports: {}", missing.join(", ")),
            })
        }
    }

    fn publish(&self, state: &mut State) {
        state.loaded.retain(|id, _| {
            state.active.values().any(|active| active == &Some(*id))
                || state.factory.values().any(|factory| factory == id)
        });
        self.exports.store(Arc::new(state.exports()));
    }
}

fn validate_name(name: &str) -> Result<(), RegistryError> {
    if name.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    }) {
        Ok(())
    } else {
        Err(RegistryError::Rejected {
            name: name.into(),
            reason: "name must use lowercase letters, digits and single hyphens".into(),
        })
    }
}
