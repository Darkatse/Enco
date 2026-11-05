//! Provider plugins turn their services' real refusals of over-long requests into overflows,
//! as the host's conversion reads them; the bodies are the services' own, from the M40 and M41
//! samples.
#![expect(clippy::unwrap_used, reason = "fixtures fail the test with context")]
use enco_core::*;
use enco_kernel::{CompleteError, ProviderRequest, Runtime};
use enco_wasm::WasmRuntime;
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

/// What `plugin` makes of `answer` to a completion request.
async fn refusal(plugin: &[u8], answer: ResponseTemplate) -> CompleteError {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(answer)
        .mount(&server)
        .await;
    let provider = WasmRuntime::new()
        .unwrap()
        .load(plugin, &PluginSettings::default())
        .await
        .unwrap()
        .completion
        .unwrap();
    let settings = ProviderSettings {
        base_url: server.uri().parse().unwrap(),
        model: "mock".into(),
        api_key_env: None,
        options: json!({}),
    };
    let request = ProviderRequest {
        messages: vec![Message::text(Role::User, "a long conversation")],
        tools: vec![],
        max_output_tokens: 8192.try_into().unwrap(),
    };
    provider
        .complete(&settings, request, None)
        .await
        .unwrap()
        .result
        .unwrap_err()
}

#[tokio::test]
async fn gemini_reads_its_limit_directly_and_through_a_proxy() {
    let gemini = include_bytes!(env!("ENCO_FACTORY_GEMINI"));
    let sentence = "The input token count exceeds the maximum number of tokens allowed 1048576.";
    let direct = refusal(
        gemini,
        ResponseTemplate::new(400).set_body_json(json!({"error": {
            "code": 400, "message": sentence, "status": "INVALID_ARGUMENT"
        }})),
    )
    .await;
    assert_eq!(
        direct,
        CompleteError::Overflow(Overflow {
            counted: None,
            accepted: Some(1_048_576),
            message: sentence.into(),
        })
    );
    let proxied = format!("Error: Current provider response failed: {sentence}");
    let through = refusal(
        gemini,
        ResponseTemplate::new(413).set_body_json(json!({"error": {
            "message": proxied, "type": "invalid_request_error"
        }})),
    )
    .await;
    assert_eq!(
        through,
        CompleteError::Overflow(Overflow {
            counted: None,
            accepted: Some(1_048_576),
            message: proxied,
        })
    );
}

#[tokio::test]
async fn chat_completions_reads_deepseek_counts_and_leaves_other_refusals_failed() {
    let chat = include_bytes!(env!("ENCO_FACTORY_OPENAI_COMPATIBLE"));
    let message = "This model's maximum context length is 1048576 tokens. However, you requested 1652195 tokens (1644003 in the messages, 8192 in the completion). Please reduce the length of the messages or completion. (request_id: 38949e8b-2196-4fd2-b11f-e7f7449f1442)";
    let deepseek = refusal(
        chat,
        ResponseTemplate::new(400).set_body_json(json!({"error": {
            "message": message, "type": "invalid_request_error",
            "param": null, "code": "invalid_request_error"
        }})),
    )
    .await;
    // The window counts the requested completion, so the input it accepts is the rest.
    assert_eq!(
        deepseek,
        CompleteError::Overflow(Overflow {
            counted: Some(1_644_003),
            accepted: Some(1_040_384),
            message: message.into(),
        })
    );
    // An HTTP 413 says the request is too large even when its body says nothing more.
    let CompleteError::Overflow(bare) = refusal(
        chat,
        ResponseTemplate::new(413).set_body_string("<html>413 Request Entity Too Large</html>"),
    )
    .await
    else {
        panic!("an HTTP 413 is an overflow")
    };
    assert_eq!((bare.counted, bare.accepted), (None, None));
    let CompleteError::Failed(failure) = refusal(
        chat,
        ResponseTemplate::new(400).set_body_json(json!({"error": {
            "message": "Invalid assistant message: content or tool_calls must be set"
        }})),
    )
    .await
    else {
        panic!("an unrelated 400 stays a failure")
    };
    assert_eq!(failure.code, code::PROVIDER_BAD_REQUEST);
}

#[tokio::test]
async fn anthropic_reads_both_counts_from_its_refusal() {
    let anthropic = include_bytes!(env!("ENCO_FACTORY_ANTHROPIC"));
    let message = "prompt is too long: 224032 tokens > 200000 maximum";
    let refused = refusal(
        anthropic,
        ResponseTemplate::new(400).set_body_json(json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": message},
            "request_id": "req_011CgiZ1eFaHSvko7AhQdyvD",
        })),
    )
    .await;
    // The output does not count against this limit, so the limit is the input it accepts.
    assert_eq!(
        refused,
        CompleteError::Overflow(Overflow {
            counted: Some(224_032),
            accepted: Some(200_000),
            message: message.into(),
        })
    );
}

#[tokio::test]
async fn responses_turns_away_an_overlong_input_without_numbers() {
    let responses = include_bytes!(env!("ENCO_FACTORY_OPENAI_RESPONSES"));
    let message = "Your input exceeds the context window of this model. Please adjust your input and try again.";
    let refused = refusal(
        responses,
        ResponseTemplate::new(400).set_body_json(json!({"error": {
            "message": message, "type": "invalid_request_error",
            "param": "input", "code": "context_length_exceeded"
        }})),
    )
    .await;
    assert_eq!(
        refused,
        CompleteError::Overflow(Overflow {
            counted: None,
            accepted: None,
            message: message.into(),
        })
    );
}
