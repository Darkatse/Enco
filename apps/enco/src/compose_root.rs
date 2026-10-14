use crate::paths::Paths;
use anyhow::Result;
use enco_host::{
    FactoryComposer, InstructionsContextSource, LIFELINE, Memories, MemoryContextSource,
    MemoryPaths, SqliteStore, StorePaths, SystemClock, config, memory_tool_specs, memory_tools,
    native_tools, plugin_tool_specs, plugin_tools,
};
use enco_kernel::{FactoryPlugin, Kernel, KernelDeps, Registry, RegistryDeps};
use enco_wasm::WasmRuntime;
use std::sync::Arc;

static OPENAI: &[u8] = include_bytes!(env!("ENCO_FACTORY_OPENAI"));
static DEEPSEEK: &[u8] = include_bytes!(env!("ENCO_FACTORY_DEEPSEEK"));
static TYPESAFE: &[u8] = include_bytes!(env!("ENCO_FACTORY_TYPESAFE"));
static WEB: &[u8] = include_bytes!(env!("ENCO_FACTORY_WEB"));

const FACTORY: [(&str, &str, &[u8]); 4] = [
    ("openai-compatible", "01M3X4HYHSE2M3523YK35VX60W", OPENAI),
    ("deepseek", "01M3X4HYHSRXVVQYXVDK5WQ9D3", DEEPSEEK),
    ("typesafe", "01M4EQVYF70Z6WGYFDKD20QF8S", TYPESAFE),
    ("web", "01K7BH08Y1C0E40KZ27QNVC89W", WEB),
];

pub(crate) struct Application {
    pub kernel: Arc<Kernel>,
    pub memories: Arc<Memories>,
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
            plugins_lock: paths.plugins_lock(),
        })
        .await?,
    );
    let clock = Arc::new(SystemClock);
    let factory = FACTORY
        .iter()
        .map(|(name, id, bytes)| {
            Ok(FactoryPlugin {
                name: (*name).into(),
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
        .chain(plugin_tool_specs())
        .chain(config::config_tool_specs(&paths.config()))
        .chain(enco_kernel::builtin_tool_specs())
        .map(|spec| spec.name)
        .collect();
    let registry = Registry::open(RegistryDeps {
        host_tool_names,
        store: store.clone(),
        runtime: Arc::new(WasmRuntime::new()?),
        factory,
        owners: enco_host::TABLE_OWNERS.into(),
        bootstrap: config::document(&paths.config())
            .await
            .map_err(|error| error.to_string()),
        clock: clock.clone(),
    })
    .await?;
    let divergence = registry
        .config_divergence(
            config::document(&paths.config())
                .await
                .map_err(|error| error.to_string()),
        )
        .await;
    if !matches!(divergence, enco_kernel::ConfigDivergence::None) {
        tracing::warn!(divergence = %serde_json::to_value(divergence)?, "config.toml differs from registered configuration");
    }
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
    tools.extend(plugin_tools(registry.clone(), paths.workspace()));
    tools.extend(config::tools(paths.config(), registry.clone()));
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
    Ok(Arc::new(Application {
        kernel,
        memories,
        channels,
        paths: paths.clone(),
    }))
}
