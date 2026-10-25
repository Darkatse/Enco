//! One Wasmtime engine for all plugins.
//!
//! A dedicated thread, the epoch clock, ticks at a short fixed interval. Guest code yields at every
//! tick, so a timeout or cancellation can stop even a guest stuck in a loop.
use crate::{WasmError, limits};
use std::{
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::JoinHandle,
};

/// Shared compiled-code engine; each logical invocation receives a fresh Store.
pub struct WasmEngine {
    pub(crate) engine: wasmtime::Engine,
    stop: Sender<()>,
    ticker: Option<JoinHandle<()>>,
}

impl WasmEngine {
    pub fn new() -> Result<Self, WasmError> {
        let cache =
            wasmtime::Cache::new(wasmtime::CacheConfig::new()).map_err(WasmError::Engine)?;
        let mut config = wasmtime::Config::new();
        config
            .cache(Some(cache))
            .wasm_component_model_async(true)
            .epoch_interruption(true);
        let engine = wasmtime::Engine::new(&config).map_err(WasmError::Engine)?;
        let clock = engine.clone();
        let (stop, stopped) = mpsc::channel();
        // Guest computation can occupy every executor worker; its clock must stay independent.
        let ticker = std::thread::Builder::new()
            .name("enco-wasm-clock".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(limits::EPOCH_TICK)
                {
                    clock.increment_epoch();
                }
            })
            .map_err(WasmError::Clock)?;
        Ok(Self {
            engine,
            stop,
            ticker: Some(ticker),
        })
    }
}

impl Drop for WasmEngine {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(ticker) = self.ticker.take()
            && ticker.join().is_err()
        {
            tracing::error!("Wasm epoch clock panicked");
        }
    }
}
