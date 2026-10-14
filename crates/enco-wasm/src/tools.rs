use crate::{
    bindings::ToolPluginPre, convert, credentials, host_imports::HostState, plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, LoadedTools, Tools};
use std::sync::Arc;

struct WasmTools {
    pre: ToolPluginPre<HostState>,
    plugin: Arc<WasmPlugin>,
}

pub(super) async fn load(
    plugin: Arc<WasmPlugin>,
    pre: ToolPluginPre<HostState>,
) -> Result<LoadedTools, Failure> {
    let declared = plugin
        .invoke(Vec::new(), Default::default(), async |mut store| {
            let instance = pre.instantiate_async(&mut store).await?;
            Ok(Ok(instance
                .enco_plugin_tools()
                .call_list(&mut store, &plugin.settings.config.to_string())
                .await?))
        })
        .await?;
    let mut names = std::collections::HashSet::new();
    let specs = declared
        .into_iter()
        .map(|spec| {
            if spec.name.is_empty() {
                return Err(convert::bad("tool name is empty"));
            }
            if !spec
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(convert::bad(format!(
                    "tool name {:?} must use ASCII letters, digits, underscores or hyphens",
                    spec.name
                )));
            }
            if !names.insert(spec.name.clone()) {
                return Err(convert::bad(format!("duplicate tool name {:?}", spec.name)));
            }
            let input_schema: serde_json::Value = serde_json::from_str(&spec.input_schema)
                .map_err(|error| convert::bad(format!("tool {} schema: {error}", spec.name)))?;
            if input_schema["type"] != "object" {
                return Err(convert::bad(format!(
                    "tool {} schema must describe an object",
                    spec.name
                )));
            }
            Ok(ToolSpec {
                name: spec.name,
                description: spec.description,
                input_schema,
                effect: convert::effect(spec.effect),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(LoadedTools {
        specs,
        adapter: Arc::new(WasmTools { pre, plugin }),
    })
}

#[async_trait]
impl Tools for WasmTools {
    async fn call(
        &self,
        ctx: CallContext,
        spec: &ToolSpec,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome {
        let credentials =
            match credentials::resolve(&self.plugin.placements, &self.plugin.settings, None) {
                Ok(credentials) => credentials,
                Err(failure) => return Outcome::Failed { failure },
            };
        let input = serde_json::Value::Object(args).to_string();
        let result = self
            .plugin
            .invoke(credentials, ctx.cancel, async |mut store| {
                let instance = self.pre.instantiate_async(&mut store).await?;
                store
                    .run_concurrent(async move |accessor| {
                        Ok(Ok(instance
                            .enco_plugin_tools()
                            .call_call(accessor, spec.name.clone(), input)
                            .await?))
                    })
                    .await?
            })
            .await
            .and_then(outcome);
        match result {
            Ok(outcome) => outcome,
            Err(failure) if spec.effect == Effect::ReadOnly => Outcome::Failed { failure },
            Err(failure) => Outcome::Unknown { failure },
        }
    }
}

fn outcome(outcome: crate::bindings::enco::plugin::types::Outcome) -> Result<Outcome, Failure> {
    use crate::bindings::enco::plugin::types::Outcome as Wire;
    Ok(match outcome {
        Wire::Ok(value) => Outcome::Ok {
            value: serde_json::from_str(&value)
                .map_err(|error| convert::bad(format!("tool result is not JSON: {error}")))?,
        },
        Wire::Failed(failure) => Outcome::Failed {
            failure: convert::failure(failure),
        },
        Wire::Unknown(failure) => Outcome::Unknown {
            failure: convert::failure(failure),
        },
    })
}
