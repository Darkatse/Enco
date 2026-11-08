//! One Wasmtime engine for all plugins.
//!
//! A dedicated thread, the epoch clock, ticks at a short fixed interval while any call may run
//! guest code, and sleeps otherwise. Guest code yields at every tick, so a timeout or cancellation
//! can stop even a guest stuck in a loop.
use crate::{WasmError, limits};
use std::{
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    thread::JoinHandle,
};

/// Shared compiled-code engine; each logical invocation receives a fresh Store.
pub struct WasmEngine {
    pub(crate) engine: wasmtime::Engine,
    clock: Arc<Clock>,
    ticker: Option<JoinHandle<()>>,
}

/// What the epoch clock waits on.
#[derive(Default)]
struct Clock {
    state: Mutex<ClockState>,
    changed: Condvar,
}

#[derive(Default)]
struct ClockState {
    /// Calls that may run guest code now.
    calls: usize,
    /// The engine is going away.
    stopping: bool,
}

impl Clock {
    fn state(&self) -> MutexGuard<'_, ClockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn tick(&self, engine: &wasmtime::Engine) {
        let mut state = self.state();
        while !state.stopping {
            state = if state.calls == 0 {
                self.changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner)
            } else {
                let (state, _) = self
                    .changed
                    .wait_timeout(state, limits::EPOCH_TICK)
                    .unwrap_or_else(PoisonError::into_inner);
                engine.increment_epoch();
                state
            };
        }
    }
}

/// Keeps the epoch clock ticking until dropped; hold it while a call may run guest code.
pub(crate) struct Ticking<'a>(&'a Clock);

impl Drop for Ticking<'_> {
    fn drop(&mut self) {
        self.0.state().calls -= 1;
    }
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
        let clock = Arc::new(Clock::default());
        let ticker = {
            let (clock, engine) = (clock.clone(), engine.clone());
            // Guest computation can occupy every executor worker; its clock must stay independent.
            std::thread::Builder::new()
                .name("enco-wasm-clock".into())
                .spawn(move || clock.tick(&engine))
                .map_err(WasmError::Clock)?
        };
        Ok(Self {
            engine,
            clock,
            ticker: Some(ticker),
        })
    }

    /// Start the clock for one call; it keeps ticking until every such call has ended.
    pub(crate) fn ticking(&self) -> Ticking<'_> {
        let mut state = self.clock.state();
        state.calls += 1;
        if state.calls == 1 {
            self.clock.changed.notify_one();
        }
        Ticking(&self.clock)
    }
}

impl Drop for WasmEngine {
    fn drop(&mut self) {
        self.clock.state().stopping = true;
        self.clock.changed.notify_one();
        if let Some(ticker) = self.ticker.take()
            && ticker.join().is_err()
        {
            tracing::error!("Wasm epoch clock panicked");
        }
    }
}
