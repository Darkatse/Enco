//! A rename moves the source directory and registers the new name as one operation.
use super::{
    Development, DevelopmentError, Failed, Operation, check_cancelled,
    error::io,
    intent::{Change, Prepared},
};
use enco_core::{CodeIntent, Initiator, IntentPath, PluginName};
use enco_kernel::{Registered, Registry};
use tokio_util::sync::CancellationToken;

impl Development {
    pub async fn rename(
        &self,
        registry: &Registry,
        name: &PluginName,
        to: &PluginName,
        initiator: Initiator,
        cancel: &CancellationToken,
    ) -> Result<Registered, Failed> {
        let _operation = self.operation.lock().await;
        let operation = Operation::Rename {
            name: name.clone(),
            to: to.clone(),
        };
        self.move_and_register(registry, name, to, initiator, cancel)
            .await
            .map_err(|error| operation.failed(error))
    }

    async fn move_and_register(
        &self,
        registry: &Registry,
        name: &PluginName,
        to: &PluginName,
        initiator: Initiator,
        cancel: &CancellationToken,
    ) -> Result<Registered, DevelopmentError> {
        check_cancelled(cancel)?;
        registry.check_rename(name, to).await?;
        let source = IntentPath::Plugin(name.clone()).locate(&self.home);
        let destination = IntentPath::Plugin(to.clone()).locate(&self.home);
        if tokio::fs::try_exists(&destination)
            .await
            .map_err(|error| io(&destination, &error))?
        {
            return Err(DevelopmentError::DirectoryExists { name: to.clone() });
        }
        let baseline = registry.registered_intent(None).await?;
        if !baseline
            .plugins
            .get(name)
            .and_then(|plugin| plugin.code.as_ref())
            .is_some_and(|code| matches!(code.intent, CodeIntent::Directory { .. }))
        {
            return Err(DevelopmentError::NoSource { name: name.clone() });
        }
        tokio::fs::rename(&source, &destination)
            .await
            .map_err(|error| io(&source, &error))?;
        let prepared = Prepared {
            plugins: [(name.clone(), Change::Rename(to.clone()))].into(),
            ..Prepared::default()
        };
        async {
            let request = self.request(&baseline, prepared, initiator).await?;
            self.register(registry, request, cancel).await
        }
        .await
        .map_err(|error| DevelopmentError::Moved(Box::new(error)))
    }
}
