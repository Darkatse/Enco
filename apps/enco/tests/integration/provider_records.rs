//! A Provider's response is recorded before it is decoded, and the answer names that record, so
//! each settlement can be traced to what the service said; the factory openai-compatible plugin
//! maps the responses, and the host sends its requests.
#![expect(clippy::unwrap_used, reason = "fixtures fail the test with context")]
use enco_core::*;
use enco_host::{SqliteStore, StorePaths};
use enco_kernel::{Answered, CompleteError, ProviderRequest, Recording, Runtime, Store};
use enco_wasm::WasmRuntime;
use serde_json::{Value, json};
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const GENERATION: GenerationId = GenerationId(7);

/// What the factory openai-compatible plugin answers when the endpoint at `base_url` responds,
/// with the store that recorded it.
async fn complete(base_url: String) -> (tempfile::TempDir, Arc<SqliteStore>, Answered) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::open(
            StorePaths {
                db: root.path().join("enco.db"),
                blobs: root.path().join("blobs"),
                artifacts: root.path().join("artifacts"),
            },
            root.path().join("workspace"),
        )
        .await
        .unwrap(),
    );
    let provider = WasmRuntime::new()
        .unwrap()
        .load(
            include_bytes!(env!("ENCO_FACTORY_OPENAI_COMPATIBLE")),
            &PluginSettings::default(),
        )
        .await
        .unwrap()
        .completion
        .unwrap();
    let settings = ProviderSettings {
        base_url: base_url.parse().unwrap(),
        model: "mock".into(),
        api_key_env: None,
        options: json!({}),
    };
    let request = ProviderRequest {
        messages: vec![Message::text(Role::User, "hello")],
        tools: vec![],
        max_output_tokens: 64.try_into().unwrap(),
    };
    let record = Recording {
        generation: GENERATION,
        store: store.clone(),
    };
    let answered = provider
        .complete(&settings, request, Some(record))
        .await
        .unwrap();
    (root, store, answered)
}

/// The service at a mock server answering `/chat/completions` with `status` and `body`.
async fn service(status: u16, body: &Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

/// The recorded call `origin` names, and the response bytes it refers to.
async fn recorded(store: &SqliteStore, origin: Option<MappingOrigin>) -> (MappingRecord, Vec<u8>) {
    let origin = origin.unwrap();
    let record: MappingRecord =
        serde_json::from_slice(&store.get_blob(&origin.call).await.unwrap()).unwrap();
    let body: ContentHash =
        serde_json::from_value(record.arguments["response"]["body"].clone()).unwrap();
    let bytes = store.get_blob(&body).await.unwrap();
    (record, bytes)
}

#[tokio::test]
async fn every_answer_names_the_recorded_response() {
    let reply = json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 1},
    });
    let refusal = json!({"error": {"message": "This model's maximum context length is 1048576 tokens. However, you requested 1652195 tokens (1644003 in the messages, 8192 in the completion). Please reduce the length of the messages or completion."}});
    let denied = json!({"error": {"message": "invalid key"}});
    for (status, body) in [(200, reply), (400, refusal), (401, denied)] {
        let server = service(status, &body).await;
        let (_root, store, answered) = complete(server.uri()).await;
        match (status, &answered.result) {
            (200, Ok(completion)) => assert_eq!(completion.message.joined_text(), "hi"),
            (400, Err(CompleteError::Overflow(overflow))) => {
                assert_eq!(overflow.counted, Some(1_644_003));
            }
            (401, Err(CompleteError::Failed(failure))) => {
                assert_eq!(failure.code, code::PROVIDER_AUTH);
            }
            (_, other) => panic!("HTTP {status} answered {other:?}"),
        }
        let (record, bytes) = recorded(&store, answered.origin).await;
        assert_eq!(record.generation, GENERATION);
        assert_eq!(record.export, "completion.decode");
        assert_eq!(record.arguments["response"]["status"], status);
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body);
    }
}

#[tokio::test]
async fn a_request_without_a_response_names_no_record_and_may_be_retried() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let (_root, _store, answered) = complete(base_url).await;
    assert!(answered.origin.is_none());
    let Err(CompleteError::Failed(failure)) = answered.result else {
        panic!("a refused connection answered {:?}", answered.result);
    };
    assert_eq!(failure.code, code::PROVIDER_NETWORK);
    assert!(failure.retryable);
}
