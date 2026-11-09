use crate::paths::Paths;
use anyhow::Result;
use enco_host::{
    InstructionsContextSource, LIFELINE, Memories, MemoryContextSource, MemoryPaths, SqliteStore,
    StorePaths, SystemClock,
    development::{Development, FactorySources, Following, Manual},
    development_tool_specs, development_tools, memory_tool_specs, memory_tools, native_tools,
    self_status_spec,
    status::HostBuild,
};
use enco_kernel::{FactoryPlugin, Kernel, KernelDeps, RegistryDeps, Store, Tool};
use enco_wasm::{WasmRuntime, plugin_limits};
use std::sync::Arc;

include!(concat!(env!("OUT_DIR"), "/sdk.rs"));

include!(concat!(env!("OUT_DIR"), "/embedded_factory.rs"));

include!(concat!(env!("OUT_DIR"), "/manual.rs"));

include!(concat!(env!("OUT_DIR"), "/factory_sources.rs"));

pub(crate) struct Application {
    pub build: HostBuild,
    pub kernel: Arc<Kernel>,
    pub memories: Arc<Memories>,
    pub development: Arc<Development>,
    pub channels: enco_host::channel::ChannelSupervisor,
    pub manual: Following,
    pub paths: Paths,
}

pub(crate) async fn compose(paths: &Paths) -> Result<Arc<Application>> {
    tokio::fs::create_dir_all(paths.workspace()).await?;
    let store = SqliteStore::open(
        StorePaths {
            db: paths.db(),
            blobs: paths.blobs(),
            artifacts: paths.artifacts(),
        },
        paths.workspace(),
    )
    .await?;
    let store = Arc::new(store);
    let clock = Arc::new(SystemClock);
    let mut tools = native_tools(store.node().await?.id)?;
    let runtime = Arc::new(WasmRuntime::new()?);
    let sdk = sdk()?;
    sdk.write(&paths.home).await?;
    let manual = Manual::new(&paths.home, MANUAL_FILES, plugin_limits(), store.clone())?;
    let development = Development::new(
        paths.home.clone(),
        sdk,
        FactorySources::new(FACTORY_SOURCES)?,
        manual,
        store.clone(),
        runtime.clone(),
    );
    let registry = development
        .open(RegistryDeps {
            host_tool_names: host_tool_names(&tools)?,
            lifeline: LIFELINE.iter().map(|s| s.to_string()).collect(),
            store: store.clone(),
            runtime,
            factory: factory()?,
            composer: FACTORY_COMPOSER.parse()?,
            owners: enco_host::TABLE_OWNERS.into(),
            clock: clock.clone(),
        })
        .await?;
    // Follow before the first write, so no view published in between is missed.
    let manual = development.manual().clone().follow(registry.clone());
    development.manual().write(&registry).await?;
    let memories = Memories::open(
        MemoryPaths {
            db: paths.memory_db(),
            index: paths.memory_index(),
        },
        registry.clone(),
        clock.clone(),
    )
    .await?;
    tools.extend(memory_tools(memories.clone())?);
    tools.extend(development_tools(development.clone(), registry.clone())?);
    let status = Arc::new(crate::status::StatusTool::default());
    tools.push(status.clone());
    let deps = KernelDeps {
        store: store.clone(),
        registry,
        home: paths.home.clone(),
        owner_instructions: Arc::new(InstructionsContextSource::owner(paths.owner_instructions())),
        profile_instructions: Arc::new(InstructionsContextSource::profile(paths.profiles())),
        memories: Arc::new(MemoryContextSource::new(memories.clone())),
        tools,
        clock: clock.clone(),
    };
    let kernel = Arc::new(Kernel::start(deps).await?);
    let build = HostBuild {
        version: env!("CARGO_PKG_VERSION").into(),
        revision: env!("ENCO_REVISION").into(),
    };
    let application = Arc::new(Application {
        channels: enco_host::channel::ChannelSupervisor::start(
            kernel.clone(),
            store,
            clock,
            build.clone(),
        ),
        build,
        kernel,
        memories,
        development,
        manual,
        paths: paths.clone(),
    });
    status
        .application
        .set(Arc::downgrade(&application))
        .map_err(|_| anyhow::anyhow!("application status was already bound"))?;
    Ok(application)
}

/// The plugins shipped in this binary.
fn factory() -> Result<Vec<FactoryPlugin>> {
    FACTORY
        .iter()
        .map(|&(name, id, artifact)| {
            Ok(FactoryPlugin {
                name: name.parse()?,
                id: id.parse()?,
                artifact,
            })
        })
        .collect()
}

/// Every tool name the host provides, which plugins may not take.
fn host_tool_names(tools: &[Arc<dyn Tool>]) -> Result<Vec<String>> {
    Ok(tools
        .iter()
        .map(|tool| tool.spec())
        .chain(memory_tool_specs()?)
        .chain(development_tool_specs()?)
        .chain([self_status_spec()])
        .chain(enco_kernel::builtin_tool_specs()?)
        .map(|spec| spec.name)
        .collect())
}
