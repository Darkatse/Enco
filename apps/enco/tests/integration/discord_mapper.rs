//! Factory Discord rendering exercises the same host contract used by live deliveries.
use crate::support::channel;
use enco_core::Outgoing;
use enco_kernel::ChannelCallError;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn discord_preserves_split_text_and_renders_it_after_the_host_boundary() {
    if channel::child(
        "discord_mapper::discord_preserves_split_text_and_renders_it_after_the_host_boundary",
    ) {
        return;
    }
    let server = MockServer::start().await;
    Mock::given(path("/api/v10/applications/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"app", "bot":{"id":"42","username":"Enco","bot":true}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/7/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"sent"})))
        .mount(&server)
        .await;
    let (_root, _store, adapter) = channel::open(
        include_bytes!(env!("ENCO_FACTORY_DISCORD")),
        "token",
        &format!("{}/api/v10", server.uri()),
    )
    .await;
    assert_eq!(adapter.identity().key(), "discord:42");
    let text = "| Name | Value |\n| --- | --- |\n| one | two |\n\n---\n```rust\nlet x = 1;\n```\n";
    let parts = adapter
        .split(Outgoing {
            text: text.into(),
            file: None,
        })
        .await
        .unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>(),
        text
    );
    assert_eq!(
        adapter
            .send("7", &parts[0])
            .await
            .unwrap()
            .message
            .as_deref(),
        Some("sent")
    );
    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
    let components = body["components"][0]["components"].as_array().unwrap();
    assert!(components.iter().any(|part| part["type"] == 14));
    assert!(
        components
            .iter()
            .filter_map(|part| part["content"].as_str())
            .any(|text| text.starts_with("```\n| Name"))
    );
    assert!(
        requests
            .iter()
            .all(|request| request.headers["authorization"] == "Bot 42:test-secret")
    );

    Mock::given(path("/api/v10/channels/7/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .with_priority(1)
        .mount(&server)
        .await;
    assert!(matches!(adapter.send("7", &parts[0]).await,
        Err(ChannelCallError::Protocol(error)) if error.unknown));
}
