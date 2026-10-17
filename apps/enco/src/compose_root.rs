use crate::paths::Paths;
use anyhow::Result;
use enco_host::{
    FactoryComposer, InstructionsContextSource, LIFELINE, Memories, MemoryContextSource,
    MemoryPaths, SqliteStore, StorePaths, SystemClock, development::Development,
    development_tool_specs, development_tools, memory_tool_specs, memory_tools, native_tools,
    self_status_spec,
};
use enco_kernel::{FactoryPlugin, Kernel, KernelDeps, RegistryDeps};
use enco_wasm::WasmRuntime;
use std::sync::Arc;

include!(concat!(env!("OUT_DIR"), "/sdk.rs"));

include!(concat!(env!("OUT_DIR"), "/embedded_factory.rs"));

pub(crate) struct Application {
    pub kernel: Arc<Kernel>,
    pub memories: Arc<Memories>,
    pub development: Arc<Development>,
    pub channels: enco_host::telegram::TelegramChannel,
    pub paths: Paths,
}

pub(crate) async fn compose(paths: &Paths) -> Result<Arc<Application>> {
    tokio::fs::create_dir_all(paths.workspace()).await?;
    let store = Arc::new(
        SqliteStore::open(StorePaths {
            db: paths.db(),
            blobs: paths.blobs(),
            artifacts: paths.artifacts(),
        })
        .await?,
    );
    let clock = Arc::new(SystemClock);
    let factory = FACTORY
        .iter()
        .map(|(name, id, bytes)| {
            Ok(FactoryPlugin {
                name: name.parse()?,
                id: id.parse()?,
                artifact: bytes.to_vec(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut tools = native_tools(paths.workspace());
    let host_tool_names = tools
        .iter()
        .map(|tool| tool.spec())
        .chain(memory_tool_specs())
        .chain(development_tool_specs())
        .chain([self_status_spec()])
        .chain(enco_kernel::builtin_tool_specs())
        .map(|spec| spec.name)
        .collect();
    let runtime = Arc::new(WasmRuntime::new()?);
    let sdk = sdk()?;
    sdk.write(&paths.home).await?;
    let development = Development::new(paths.home.clone(), sdk, store.clone(), runtime.clone());
    let registry = development
        .open(RegistryDeps {
            host_tool_names,
            store: store.clone(),
            runtime,
            factory,
            owners: enco_host::TABLE_OWNERS.into(),
            clock: clock.clone(),
        })
        .await?;
    let memories = Memories::open(
        MemoryPaths {
            db: paths.memory_db(),
            index: paths.memory_index(),
        },
        registry.clone(),
        clock.clone(),
    )
    .await?;
    tools.extend(memory_tools(memories.clone()));
    tools.extend(development_tools(development.clone(), registry.clone()));
    let status = Arc::new(crate::status::StatusTool::default());
    tools.push(status.clone());
    let kernel = Kernel::start(KernelDeps {
        store,
        registry,
        composer: Arc::new(FactoryComposer::new(
            paths.workspace(),
            paths.instructions(),
        )),
        context: vec![
            Arc::new(InstructionsContextSource::new(paths.instructions())),
            Arc::new(MemoryContextSource::new(memories.clone())),
        ],
        tools,
        lifeline: LIFELINE.iter().map(|s| s.to_string()).collect(),
        clock: clock.clone(),
    })
    .await?;
    let kernel = Arc::new(kernel);
    let channels = enco_host::telegram::TelegramChannel::start(kernel.clone(), clock);
    let application = Arc::new(Application {
        kernel,
        memories,
        development,
        channels,
        paths: paths.clone(),
    });
    status
        .application
        .set(Arc::downgrade(&application))
        .map_err(|_| anyhow::anyhow!("application status was already bound"))?;
    Ok(application)
}
