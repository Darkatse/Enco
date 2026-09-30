use crate::{config::Config, paths::Paths};
use anyhow::{Result, bail};
use enco_host::{
    FactoryComposer, InstructionsContextSource, LIFELINE, Memories, MemoryContextSource,
    MemoryPaths, SqliteStore, SystemClock, memory_tools, native_tools,
};
use enco_kernel::{Budget, Kernel, KernelConfig, KernelDeps, Store};
use enco_wasm::{WasmEngine, WasmProvider};
use std::sync::Arc;

static OPENAI: &[u8] = include_bytes!(env!("ENCO_FACTORY_OPENAI"));
static DEEPSEEK: &[u8] = include_bytes!(env!("ENCO_FACTORY_DEEPSEEK"));

fn factory(name: &str) -> Result<&'static [u8]> {
    match name {
        "openai-compatible" => Ok(OPENAI),
        "deepseek" => Ok(DEEPSEEK),
        _ => bail!("unknown factory plugin {name}; choose openai-compatible or deepseek"),
    }
}

pub(crate) struct Application {
    pub kernel: Arc<Kernel>,
    pub memories: Arc<Memories>,
    pub channels: Vec<enco_host::channel::Channel>,
}

pub(crate) async fn compose(paths: &Paths) -> Result<Arc<Application>> {
    let (config, embedding) = Config::load(&paths.config()).await?;
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
    let store = Arc::new(SqliteStore::open(paths.db(), paths.blobs()).await?);
    let provider_bytes = factory(&config.provider.plugin)?;
    let embedding_bytes = factory(&config.embedding.plugin)?;
    store.put_blob(provider_bytes).await?;
    store.put_blob(embedding_bytes).await?;
    let engine = Arc::new(WasmEngine::new()?);
    let provider = Arc::new(
        WasmProvider::new(engine.clone(), provider_bytes, config.provider.settings()?).await?,
    );
    let embedder =
        Arc::new(WasmProvider::new(engine, embedding_bytes, config.embedding.settings()?).await?);
    let clock = Arc::new(SystemClock);
    let memories = Memories::open(
        MemoryPaths {
            db: paths.memory_db(),
            index: paths.memory_index(),
        },
        embedding,
        embedder,
        clock.clone(),
    )
    .await?;
    let mut tools = native_tools(paths.workspace());
    tools.extend(memory_tools(memories.clone()));
    let kernel = Kernel::start(
        KernelDeps {
            store,
            provider,
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
