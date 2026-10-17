//! All management entries prepare intent here, then let the registry accept it exactly once.
use super::{
    Applied, Development, DevelopmentError, check_cancelled,
    git::Snapshot,
    intent::{Change, Prepared},
};
use enco_kernel::{
    PluginChange, Registered, RegisteredIntent, RegistrationRequest, Registry, RegistryDeps,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Development {
    /// Restore first, repair the chain anchor, then offer host changes through ordinary registration.
    pub async fn open(
        &self,
        deps: RegistryDeps,
        host_version: &str,
    ) -> Result<Arc<Registry>, DevelopmentError> {
        let _operation = self.operation.lock().await;
        let cancel = CancellationToken::new();
        let initial_cancel = cancel.clone();
        let registry = Registry::open(deps, |baseline| async move {
            let prepared = self.prepare(&baseline, &initial_cancel).await?;
            self.request(&baseline, prepared, "first boot").await
        })
        .await?;
        let baseline = registry.registered_intent().await;
        if let Some(record) = &baseline.base {
            self.anchor(&record.revision).await;
        }
        let following: Vec<_> = baseline
            .plugins
            .iter()
            .filter(|(_, plugin)| {
                plugin
                    .factory
                    .as_ref()
                    .is_some_and(|factory| factory.follow)
            })
            .collect();
        if !following.is_empty() {
            let prepared = Prepared {
                plugins: following
                    .iter()
                    .map(|(name, _)| ((*name).clone(), Change::Factory))
                    .collect(),
                ..Prepared::default()
            };
            let result = async {
                self.git.check().await?;
                self.register(
                    &registry,
                    &baseline,
                    prepared,
                    &format!("host follow {host_version}"),
                    &cancel,
                )
                .await
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(%error, "factory follow rejected; keeping registered behavior");
                for (_, plugin) in following {
                    self.follow_rejections
                        .write()
                        .await
                        .insert(plugin.id, error.to_string());
                }
            }
        }
        Ok(registry)
    }

    /// Capture source and configuration once, then offer the complete change to the registry.
    pub async fn apply(
        &self,
        registry: &Registry,
        cancel: &CancellationToken,
    ) -> Result<Applied, DevelopmentError> {
        let _operation = self.operation.lock().await;
        let baseline = registry.registered_intent().await;
        let mut prepared = self.prepare(&baseline, cancel).await?;
        check_cancelled(cancel)?;
        let builds = std::mem::take(&mut prepared.builds);
        let registered = if prepared.changed() {
            Some(
                self.register(registry, &baseline, prepared, "self_apply", cancel)
                    .await?,
            )
        } else {
            None
        };
        Ok(Applied { registered, builds })
    }

    /// Direct artifacts share acceptance and history while leaving file intent unchanged.
    pub async fn deploy(
        &self,
        registry: &Registry,
        name: String,
        bytes: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<Registered, DevelopmentError> {
        let _operation = self.operation.lock().await;
        self.git.check().await?;
        let baseline = registry.registered_intent().await;
        let prepared = Prepared {
            plugins: [(name, Change::Artifact(bytes))].into(),
            ..Prepared::default()
        };
        self.register(registry, &baseline, prepared, "artifact", cancel)
            .await
    }

    pub(super) async fn register(
        &self,
        registry: &Registry,
        baseline: &RegisteredIntent,
        prepared: Prepared,
        source: &str,
        cancel: &CancellationToken,
    ) -> Result<Registered, DevelopmentError> {
        check_cancelled(cancel)?;
        let request = self.request(baseline, prepared, source).await?;
        check_cancelled(cancel)?;
        let mut registered = registry.register(request).await?;
        for (id, plugin) in &registered.registration.code {
            if plugin.intent.is_some() {
                self.follow_rejections.write().await.remove(id);
            }
        }
        registered
            .warnings
            .extend(self.anchor(&registered.registration.revision).await);
        Ok(registered)
    }

    /// The only writer of chain commits; bootstrap also uses this before its first view exists.
    async fn request(
        &self,
        baseline: &RegisteredIntent,
        prepared: Prepared,
        source: &str,
    ) -> Result<RegistrationRequest, DevelopmentError> {
        let previous = baseline.base.as_ref();
        let base = previous.map(|record| record.revision.as_str());
        let parent = match base {
            Some(revision) => Some(revision.to_owned()),
            None => self.git.reference().await?,
        };
        let mut message = format!("{source}\n");
        let mut plugins = std::collections::BTreeMap::new();
        let mut snapshots = Vec::new();
        let configuration = prepared.configuration.map(|(bytes, document)| {
            snapshots.push(Snapshot::File {
                path: "config.toml".into(),
                bytes,
            });
            message.push_str("config.toml\n");
            document
        });
        for (name, change) in prepared.plugins {
            let path = format!("plugins/{name}");
            let (change, description) = match change {
                Change::Code { artifact, tree } => {
                    snapshots.push(Snapshot::Tree { path, tree });
                    (PluginChange::Code(artifact), "directory".into())
                }
                Change::Factory => {
                    snapshots.push(Snapshot::Removed(path));
                    (PluginChange::Factory, "factory".into())
                }
                Change::Remove => {
                    snapshots.push(Snapshot::Removed(path));
                    (PluginChange::Remove, "removed".into())
                }
                Change::Artifact(bytes) => (PluginChange::Artifact(bytes), "artifact".into()),
                Change::Rename { to, tree } => {
                    snapshots.push(Snapshot::Removed(path));
                    snapshots.push(Snapshot::Tree {
                        path: format!("plugins/{to}"),
                        tree,
                    });
                    let description = format!("renamed to {to}");
                    (PluginChange::Rename(to), description)
                }
            };
            message.push_str(&format!("{name}: {description}\n"));
            plugins.insert(name, change);
        }
        let tree = self.git.compose(base, &snapshots).await?;
        let revision = self.git.commit(parent.as_deref(), &tree, &message).await?;
        Ok(RegistrationRequest {
            revision,
            base: previous.map(|record| record.id),
            configuration,
            plugins,
        })
    }

    async fn anchor(&self, revision: &str) -> Option<String> {
        match self.git.anchor(revision).await {
            Ok(()) => None,
            Err(error) => {
                let warning =
                    format!("registration accepted; chain reference update failed: {error}");
                tracing::warn!("{warning}");
                Some(warning)
            }
        }
    }
}
