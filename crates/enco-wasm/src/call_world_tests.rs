//! Verifies the host state visible to compositions, declarations and probes.
use super::{CallWorld, Network};
use enco_core::{DateTime, RoundId, Utc};
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{
    WasiCtx,
    clocks::WasiClocksCtxView,
    p2::bindings::{
        cli::environment,
        clocks::{monotonic_clock, wall_clock},
        filesystem::preopens,
        random::{insecure, insecure_seed, random},
    },
};

fn composer(now: DateTime<Utc>, round: RoundId) -> WasiCtx {
    let (network, config, wasi) = CallWorld::Composer { now, round }
        .configure(&serde_json::json!({"text": "configured"}))
        .expect("composer world");
    assert!(matches!(network, Network::Closed));
    assert_eq!(config, r#"{"text":"configured"}"#);
    wasi
}

#[test]
fn composer_wasi_is_frozen_and_seeded_by_round() {
    let now = Utc::now();
    let round = RoundId::new();
    let mut first = composer(now, round);
    let mut second = composer(now, round);
    let mut next_round = composer(now, RoundId::new());
    assert_ne!(
        insecure_seed::Host::insecure_seed(first.random()).expect("seed"),
        insecure_seed::Host::insecure_seed(next_round.random()).expect("seed")
    );
    for _ in 0..3 {
        let secure = random::Host::get_random_bytes(first.random(), 64).expect("random");
        assert_eq!(
            secure,
            random::Host::get_random_bytes(second.random(), 64).expect("random")
        );
        assert_ne!(
            secure,
            random::Host::get_random_bytes(next_round.random(), 64).expect("random")
        );
        let insecure =
            insecure::Host::get_insecure_random_bytes(first.random(), 64).expect("insecure");
        assert_eq!(
            insecure,
            insecure::Host::get_insecure_random_bytes(second.random(), 64).expect("insecure")
        );
        assert_ne!(
            insecure,
            insecure::Host::get_insecure_random_bytes(next_round.random(), 64).expect("insecure")
        );
        assert_eq!(
            insecure_seed::Host::insecure_seed(first.random()).expect("seed"),
            insecure_seed::Host::insecure_seed(second.random()).expect("seed")
        );
    }
    let mut table = ResourceTable::new();
    let mut clocks = WasiClocksCtxView {
        ctx: first.clocks(),
        table: &mut table,
    };
    for _ in 0..3 {
        let wall = wall_clock::Host::now(&mut clocks).expect("wall clock");
        assert_eq!(wall.seconds, now.timestamp() as u64);
        assert_eq!(wall.nanoseconds, now.timestamp_subsec_nanos());
        assert_eq!(
            monotonic_clock::Host::now(&mut clocks).expect("monotonic clock"),
            0
        );
    }
    assert!(
        environment::Host::get_environment(&mut wasmtime_wasi::cli::WasiCliCtxView {
            ctx: first.cli(),
            table: &mut table
        })
        .expect("environment")
        .is_empty()
    );
    assert!(
        preopens::Host::get_directories(&mut wasmtime_wasi::filesystem::WasiFilesystemCtxView {
            ctx: first.filesystem(),
            table: &mut table
        })
        .expect("preopens")
        .is_empty()
    );
}

#[test]
fn declarations_and_probes_have_no_network() {
    let registered = serde_json::json!({"owner": "value"});
    let (network, config, _) = CallWorld::Declaration
        .configure(&registered)
        .expect("declaration world");
    assert!(matches!(network, Network::Closed));
    assert_eq!(config, registered.to_string());
    let (network, config, _) = CallWorld::Probe
        .configure(&registered)
        .expect("probe world");
    assert!(matches!(network, Network::Closed));
    assert_eq!(config, "{}");
}
