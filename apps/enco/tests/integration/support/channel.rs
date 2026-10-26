//! Real factory channel fixtures use a child process for their fixed transport credential.
use enco_core::{GenerationId, PluginSettings};
use enco_host::{SqliteStore, StorePaths};
use enco_kernel::{ChannelAdapter, ChannelCallError, ChannelFault, ChannelSettings, Runtime};
use serde_json::json;
use std::sync::Arc;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

pub fn child(test: &str) -> bool {
    if std::env::var_os("ENCO_CHANNEL_TEST_CHILD").is_some() {
        return false;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("ENCO_CHANNEL_TEST_CHILD", "1")
        .env("ENCO_CHANNEL_FIXTURE_TOKEN", "42:test-secret")
        .status()
        .unwrap();
    assert!(status.success());
    true
}

pub async fn telegram(
    server: &MockServer,
) -> (tempfile::TempDir, Arc<SqliteStore>, Arc<dyn ChannelAdapter>) {
    Mock::given(path("/bot42:test-secret/getMe"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"id": 42, "username": "FixtureBot"}})),
        )
        .mount(server)
        .await;
    open(&server.uri()).await
}

pub async fn open(base: &str) -> (tempfile::TempDir, Arc<SqliteStore>, Arc<dyn ChannelAdapter>) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::open(StorePaths {
            db: root.path().join("enco.db"),
            blobs: root.path().join("blobs"),
            artifacts: root.path().join("artifacts"),
        })
        .await
        .unwrap(),
    );
    let loaded = enco_wasm::WasmRuntime::new()
        .unwrap()
        .load(
            include_bytes!(env!("ENCO_FACTORY_TELEGRAM")),
            &PluginSettings::default(),
        )
        .await
        .unwrap();
    let adapter = loaded
        .channel
        .unwrap()
        .open(
            &ChannelSettings {
                base_url: base.into(),
                secrets: [("token".into(), "ENCO_CHANNEL_FIXTURE_TOKEN".into())].into(),
            },
            GenerationId(31),
            store.clone(),
        )
        .await
        .unwrap();
    (root, store, adapter)
}

pub fn fault<T>(result: Result<T, ChannelCallError>) -> ChannelFault {
    match result {
        Err(ChannelCallError::Protocol(fault)) => *fault,
        Err(error) => panic!("expected protocol failure: {error}"),
        Ok(_) => panic!("expected protocol failure"),
    }
}
