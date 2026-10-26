//! Gives each invocation the host state its role permits, including the Round's deterministic WASI.
use crate::{credentials::CallCredential, host_imports::Network};
use enco_core::{DateTime, RoundId, Utc};
use rand::{SeedableRng, rngs::StdRng};
use std::time::Duration;
use wasmtime_wasi::{HostMonotonicClock, HostWallClock, WasiCtx};

pub(crate) enum CallWorld {
    /// Serving a call: the plugin's configuration, and network carrying these credentials.
    Regular(Vec<CallCredential>),
    /// Mapping recorded protocol input: fixed clocks and random seed, no configuration or network.
    Mapper,
    /// Declaring what the generation offers: its configuration, without network, so a reload
    /// never depends on an outside service.
    Declaration,
    /// Checking the code alone: no configuration and no network.
    Probe,
    /// Composing within a Round: its configuration, the Round's clock and randomness, no network.
    Composer { now: DateTime<Utc>, round: RoundId },
}

impl CallWorld {
    pub(crate) fn configure(
        self,
        config: &serde_json::Value,
    ) -> Result<(Network, String, WasiCtx), enco_core::Failure> {
        let mut wasi = WasiCtx::builder();
        if !matches!(self, Self::Mapper) {
            wasi.inherit_stderr();
        }
        let (network, config) = match self {
            Self::Regular(credentials) => (Network::open(credentials), config.to_string()),
            Self::Declaration => (Network::Closed, config.to_string()),
            Self::Probe => (Network::Closed, "{}".into()),
            Self::Mapper => {
                wasi.wall_clock(RoundClock(Duration::ZERO))
                    .monotonic_clock(FrozenClock)
                    .secure_random(StdRng::from_seed([0; 32]))
                    .insecure_random(StdRng::from_seed([0; 32]))
                    .insecure_random_seed(0);
                (Network::Closed, "{}".into())
            }
            Self::Composer { now, round } => {
                let seconds = now
                    .timestamp()
                    .try_into()
                    .map_err(|_| crate::convert::bad("Round time precedes the Unix epoch"))?;
                let seed = round.as_u128();
                let mut random_seed = [0; 32];
                random_seed[..16].copy_from_slice(&seed.to_le_bytes());
                random_seed[16..].copy_from_slice(&seed.to_be_bytes());
                wasi.wall_clock(RoundClock(Duration::new(
                    seconds,
                    now.timestamp_subsec_nanos(),
                )))
                .monotonic_clock(FrozenClock)
                .secure_random(StdRng::from_seed(random_seed))
                .insecure_random(StdRng::from_seed(random_seed))
                .insecure_random_seed(seed);
                (Network::Closed, config.to_string())
            }
        };
        Ok((network, config, wasi.build()))
    }
}

struct RoundClock(Duration);

impl HostWallClock for RoundClock {
    fn resolution(&self) -> Duration {
        Duration::from_nanos(1)
    }
    fn now(&self) -> Duration {
        self.0
    }
}

struct FrozenClock;

impl HostMonotonicClock for FrozenClock {
    fn resolution(&self) -> u64 {
        1
    }
    fn now(&self) -> u64 {
        0
    }
}

#[cfg(test)]
#[path = "call_world_tests.rs"]
mod tests;
