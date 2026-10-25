//! Every management entry prepares content here, then the registry accepts its references once.
use super::{
    Applied, Development, DevelopmentError, Failed, Operation, check_cancelled,
    intent::{Change, Prepared},
};
use enco_core::{Initiator, PluginName};
use enco_kernel::{
    ConfigurationSource, PluginChange, Registered, RegisteredIntent, RegistrationRequest, Registry,
    RegistryDeps, RegistryError, Store,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Development {
    /// Restore accepted behavior, then offer host changes through ordinary registration.
    pub async fn open(&self, deps: RegistryDeps) -> Result<Arc<Registry>, Failed> {
        let _operation = self.operation.lock().await;
        self.start(deps)
            .await
            .map_err(|error| Operation::Startup.failed(error))
    }

    async fn start(&self, deps: RegistryDeps) -> Result<Arc<Registry>, DevelopmentError> {
        let cancel = CancellationToken::new();
        let initial_cancel = cancel.clone();
        let registry = Registry::open(
            deps,
            |baseline| async move {
                let prepared = self.prepare(&baseline, &initial_cancel).await?;
                self.request(&baseline, prepared, Initiator::Host).await
            },
            DevelopmentError::from,
        )
        .await?;
        let baseline = registry.registered_intent(None).await?;
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
        if following.is_empty() {
            return Ok(registry);
        }
        let prepared = Prepared {
            plugins: following
                .iter()
                .map(|(name, _)| ((*name).clone(), Change::Factory))
                .collect(),
            ..Prepared::default()
        };
        let request = self.request(&baseline, prepared, Initiator::Host).await?;
        // A rejected host update keeps registered behavior; any other failure stops startup.
        match self.register(&registry, request, &cancel).await {
            Ok(_) => {}
            Err(DevelopmentError::Registry(RegistryError::Rejected(causes))) => {
                tracing::warn!(
                    ?causes,
                    "factory follow rejected; keeping registered behavior"
                );
                let mut rejections = self.follow_rejections.write().await;
                for (_, plugin) in following {
                    rejections.insert(plugin.id, causes.clone());
                }
            }
            Err(error) => return Err(error),
        }
        Ok(registry)
    }

    /// Capture source and configuration once, then offer the complete change to the registry.
    pub async fn apply(
        &self,
        registry: &Registry,
        initiator: Initiator,
        cancel: &CancellationToken,
    ) -> Result<Applied, Failed> {
        let _operation = self.operation.lock().await;
        self.register_home(registry, initiator, cancel)
            .await
            .map_err(|error| Operation::Apply.failed(error))
    }

    async fn register_home(
        &self,
        registry: &Registry,
        initiator: Initiator,
        cancel: &CancellationToken,
    ) -> Result<Applied, DevelopmentError> {
        let baseline = registry.registered_intent(None).await?;
        let mut prepared = self.prepare(&baseline, cancel).await?;
        check_cancelled(cancel)?;
        let builds = std::mem::take(&mut prepared.builds);
        let text_only = std::mem::take(&mut prepared.text_only);
        let registered = if prepared.changed() {
            let request = self.request(&baseline, prepared, initiator).await?;
            Some(self.register(registry, request, cancel).await?)
        } else {
            None
        };
        Ok(Applied {
            registered,
            builds,
            text_only,
        })
    }

    /// Direct artifacts share acceptance and history while leaving file intent unchanged.
    pub async fn deploy(
        &self,
        registry: &Registry,
        name: PluginName,
        bytes: Vec<u8>,
        initiator: Initiator,
        cancel: &CancellationToken,
    ) -> Result<Registered, Failed> {
        let _operation = self.operation.lock().await;
        async {
            let baseline = registry.registered_intent(None).await?;
            let prepared = Prepared {
                plugins: [(name, Change::Artifact(bytes))].into(),
                ..Prepared::default()
            };
            let request = self.request(&baseline, prepared, initiator).await?;
            self.register(registry, request, cancel).await
        }
        .await
        .map_err(|error| Operation::Deploy.failed(error))
    }

    pub(super) async fn register(
        &self,
        registry: &Registry,
        request: RegistrationRequest,
        cancel: &CancellationToken,
    ) -> Result<Registered, DevelopmentError> {
        check_cancelled(cancel)?;
        let registered = registry.register(request).await?;
        {
            let mut rejections = self.follow_rejections.write().await;
            for (id, plugin) in &registered.registration.code {
                if plugin.intent.is_some() {
                    rejections.remove(id);
                }
            }
        }
        // The registration stands either way; the manual follows the registry and retries.
        if let Err(error) = self.manual.write(registry).await {
            tracing::error!(%error, "manual not rewritten after a registration");
        }
        Ok(registered)
    }

    /// Persist the prepared bytes before their references are admitted in one transaction.
    pub(super) async fn request(
        &self,
        baseline: &RegisteredIntent,
        prepared: Prepared,
        initiator: Initiator,
    ) -> Result<RegistrationRequest, DevelopmentError> {
        let configuration = match prepared.configuration {
            Some(configuration) => Some(ConfigurationSource {
                document: configuration.document,
                source: self.store.put_blob(&configuration.bytes).await?,
            }),
            None => None,
        };
        let mut plugins = std::collections::BTreeMap::new();
        for (name, change) in prepared.plugins {
            let change = match change {
                Change::Code { artifact, tree } => PluginChange::Code {
                    source: tree.store(self.store.as_ref()).await?,
                    artifact,
                },
                Change::Factory => PluginChange::Factory,
                Change::Remove => PluginChange::Remove,
                Change::Artifact(bytes) => PluginChange::Artifact(bytes),
                Change::Rename(to) => PluginChange::Rename(to),
            };
            plugins.insert(name, change);
        }
        Ok(RegistrationRequest {
            initiator,
            base: baseline.registration(),
            configuration,
            plugins,
        })
    }
}
