use crate::{config::Config, paths::Paths};
use anyhow::Result;
use enco_host::{
    FactoryComposer, InstructionsContextSource, LIFELINE, Memories, MemoryContextSource,
    MemoryPaths, SqliteStore, StorePaths, SystemClock, memory_tools, native_tools, plugin_tools,
};
use enco_kernel::{
    Budget, FactoryPlugin, Interface, Kernel, KernelConfig, KernelDeps, Registry, RegistryDeps, Use,
};
use enco_wasm::WasmRuntime;
use std::sync::Arc;

static OPENAI: &[u8] = include_bytes!(env!("ENCO_FACTORY_OPENAI"));
static DEEPSEEK: &[u8] = include_bytes!(env!("ENCO_FACTORY_DEEPSEEK"));

const FACTORY: [(&str, &str, &[u8]); 2] = [
    ("openai-compatible", "01M3X4HYHSE2M3523YK35VX60W", OPENAI),
    ("deepseek", "01M3X4HYHSRXVVQYXVDK5WQ9D3", DEEPSEEK),
];

pub(crate) struct Application {
    pub kernel: Arc<Kernel>,
    pub memories: Arc<Memories>,
    pub channels: Vec<enco_host::channel::Channel>,
}

pub(crate) async fn compose(paths: &Paths) -> Result<Arc<Application>> {
    let (config, profile, embedding) = Config::load(&paths.config()).await?;
    let channel = config
        .telegram
        .as_ref()
        .map(|config| Ok::<_, anyhow::Error>((config.adapter()?, config.owner_user_id.to_string())))
        .transpose()?;
    let kernel_config = KernelConfig::new(
        Budget {
            context_tokens: config.context.window_tokens,
            max_output_tokens: config.context.max_output_tokens,
        },
        config.run.max_rounds,
    )?;
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
    let registry = Registry::open(RegistryDeps {
        store: store.clone(),
        runtime: Arc::new(WasmRuntime::new()?),
        factory,
        wiring: vec![
            Use {
                user: "profile default.reply".into(),
                plugin: profile.reply.plugin.clone(),
                interface: Interface::Completion,
            },
            Use {
                user: "profile default.compaction".into(),
                plugin: profile.compaction.plugin.clone(),
                interface: Interface::Completion,
            },
            Use {
                user: "embedding".into(),
                plugin: embedding.plugin.clone(),
                interface: Interface::Embedding,
            },
        ],
        clock: clock.clone(),
    })
    .await?;
    let memories = Memories::open(
        MemoryPaths {
            db: paths.memory_db(),
            index: paths.memory_index(),
        },
        embedding,
        registry.clone(),
        clock.clone(),
    )
    .await?;
    let mut tools = native_tools(paths.workspace());
    tools.extend(memory_tools(memories.clone()));
    tools.extend(plugin_tools(registry.clone(), paths.workspace()));
    let kernel = Kernel::start(
        KernelDeps {
            store,
            registry,
            profile,
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
        },
        kernel_config,
    )
    .await?;
    let kernel = Arc::new(kernel);
    let mut channels = Vec::new();
    if let Some((adapter, owner_id)) = channel {
        channels.push(enco_host::channel::Channel::start(
            kernel.clone(),
            Arc::new(adapter),
            owner_id,
            clock,
        ));
    }
    Ok(Arc::new(Application {
        kernel,
        memories,
        channels,
    }))
}
