//! Loads the composer's options declaration once and adapts pure compositions to the kernel port.
use crate::{
    bindings::ComposerPluginPre, call_world::CallWorld, composer_convert, convert,
    host_imports::HostState, plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::{Failure, InputSchema};
use enco_kernel::{ComposeInput, Composer, ComposerOptions, Composition, LoadedComposer};
use std::sync::Arc;

struct WasmComposer {
    pre: ComposerPluginPre<HostState>,
    plugin: Arc<WasmPlugin>,
}

pub(super) async fn load(
    plugin: Arc<WasmPlugin>,
    pre: ComposerPluginPre<HostState>,
) -> Result<LoadedComposer, Failure> {
    let declared = plugin
        .invoke(
            CallWorld::Declaration,
            Default::default(),
            async |mut store| {
                let instance = pre.instantiate_async(&mut store).await?;
                Ok(Ok(instance
                    .enco_plugin_composer()
                    .call_options_schema(&mut store, &plugin.settings.config.to_string())
                    .await?))
            },
        )
        .await?;
    let schema = serde_json::from_str(&declared)
        .map_err(|error| convert::bad(format!("composer options schema is not JSON: {error}")))?;
    let schema = InputSchema::parse(&schema)
        .map_err(|error| convert::bad(format!("composer options schema: {error}")))?;
    schema.accept(serde_json::Map::new()).map_err(|error| {
        convert::bad(format!(
            "composer options must accept an empty object: {error}"
        ))
    })?;
    Ok(LoadedComposer {
        schema,
        adapter: Arc::new(WasmComposer { pre, plugin }),
    })
}

#[async_trait]
impl Composer for WasmComposer {
    async fn compose(
        &self,
        options: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        let world = CallWorld::Composer {
            now: input.now,
            round: input.round,
        };
        let input = composer_convert::input(input)?;
        let options = serde_json::Value::Object(options.clone()).to_string();
        let result = self
            .plugin
            .invoke(world, Default::default(), async |mut store| {
                let instance = self.pre.instantiate_async(&mut store).await?;
                let result = instance
                    .enco_plugin_composer()
                    .call_compose(&mut store, &options, &input)
                    .await?;
                Ok(result.map_err(|mut failure| {
                    if failure.code != enco_core::code::CONTEXT_OVERFLOW {
                        failure.code = enco_core::code::COMPOSE_FAILED.into();
                    }
                    failure
                }))
            })
            .await?;
        composer_convert::composition(result)
    }
}
