use enco_core::*;
use enco_kernel::{ProviderRequest, Runtime};
use enco_wasm::WasmRuntime;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn native_gemini_preserves_tool_parts_and_embeds_in_input_order() {
    let server = MockServer::start().await;
    let content = json!({"role": "model", "parts": [
        {"functionCall": {"id": "one", "name": "read", "args": {}}, "thoughtSignature": "opaque-signature"},
        {"functionCall": {"id": "two", "name": "read", "args": {"second": true}}}
    ]});
    mock_completion(&server, content.clone(), "STOP").await;
    let loaded = WasmRuntime::new()
        .unwrap()
        .load(
            include_bytes!(env!("ENCO_FACTORY_GEMINI")),
            &PluginSettings::default(),
        )
        .await
        .unwrap();
    let provider = loaded.completion.unwrap();
    let settings = ProviderSettings {
        base_url: server.uri(),
        model: "mock".into(),
        api_key_env: None,
        options: json!({"generationConfig": {"thinkingConfig": {"thinkingLevel": "LOW"}}}),
    };
    let first = provider
        .complete(
            &settings,
            ProviderRequest {
                messages: vec![Message {
                    role: Role::User,
                    parts: vec![Part::Text {
                        text: "read both".into(),
                    }],
                }],
                tools: vec![],
                max_output_tokens: Some(100),
            },
        )
        .await
        .unwrap();
    assert_eq!(first.usage.output_tokens, 7);
    let messages = tool_history(first.message).unwrap();
    server.reset().await;
    mock_completion(
        &server,
        json!({"role": "model", "parts": [{"text": "done"}]}),
        "STOP",
    )
    .await;
    let answer = provider
        .complete(
            &settings,
            ProviderRequest {
                messages,
                tools: vec![],
                max_output_tokens: Some(100),
            },
        )
        .await
        .unwrap();
    assert_eq!(answer.message.joined_text(), "done");
    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["contents"][0], content);
    assert_eq!(
        body["contents"][1]["parts"][1]["functionResponse"]["id"],
        "two"
    );
    assert_eq!(
        body["contents"][1]["parts"][0]["functionResponse"]["parts"][0]["inlineData"],
        json!({"mimeType": "image/png", "data": "AQID"})
    );
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 100);
    Mock::given(method("POST"))
        .and(path("/models/mock:batchEmbedContents"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"embeddings": [{"values": [1.0, 0.0]}, {"values": [0.0, 1.0]}]}),
            ),
        )
        .mount(&server)
        .await;
    let embeddings = loaded
        .embedding
        .unwrap()
        .embed(
            &ProviderSettings {
                options: json!({"outputDimensionality": 2}),
                ..settings
            },
            vec!["first".into(), "second".into()],
        )
        .await
        .unwrap();
    assert_eq!(embeddings, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
}

async fn mock_completion(server: &MockServer, content: Value, finish: &str) {
    Mock::given(method("POST")).and(path("/models/mock:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": content, "finishReason": finish}],
            "usageMetadata": {"promptTokenCount": 20, "candidatesTokenCount": 3, "thoughtsTokenCount": 4}
        }))).mount(server).await;
}

fn tool_history(message: Message) -> Option<Vec<Message<ImageData>>> {
    let mut messages: Vec<Message<ImageData>> = vec![Message {
        role: Role::Assistant,
        parts: message
            .parts
            .into_iter()
            .map(|part| {
                Some(match part {
                    Part::Text { text } => Part::Text { text },
                    Part::ToolCall(call) => Part::ToolCall(call),
                    Part::ToolResult(result) => Part::ToolResult(result),
                    Part::Extension(extension) => Part::Extension(extension),
                    Part::Image(_) => return None,
                })
            })
            .collect::<Option<Vec<_>>>()?,
    }];
    let calls = messages[0]
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for call in calls {
        messages.push(Message {
            role: Role::Tool,
            parts: vec![
                Part::ToolResult(ToolResult {
                    call: call.id,
                    provider_id: call.provider_id,
                    content: "read".into(),
                    is_error: false,
                }),
                Part::Image(ImageData {
                    format: ImageFormat::Png,
                    data: vec![1, 2, 3],
                }),
            ],
        });
    }
    Some(messages)
}
