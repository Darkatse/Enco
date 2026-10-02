use crate::{WasmError, limits};

/// Shared compiled-code engine; each logical invocation receives a fresh Store.
pub struct WasmEngine {
    pub(crate) engine: wasmtime::Engine,
    ticker: tokio::task::JoinHandle<()>,
}

impl WasmEngine {
    pub fn new() -> Result<Self, WasmError> {
        let mut config = wasmtime::Config::new();
        config
            .wasm_component_model_async(true)
            .epoch_interruption(true);
        let engine = wasmtime::Engine::new(&config).map_err(WasmError::Engine)?;
        let clock = engine.clone();
        let ticker = tokio::spawn(async move {
            let mut tick = tokio::time::interval(limits::EPOCH_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                clock.increment_epoch();
            }
        });
        Ok(Self { engine, ticker })
    }
}

impl Drop for WasmEngine {
    fn drop(&mut self) {
        self.ticker.abort();
    }
}
