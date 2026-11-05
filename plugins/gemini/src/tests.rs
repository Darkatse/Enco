use super::{Plugin, completion::Guest as _, request, response};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpResponse, Request, Settings, StopReason,
    types::{Failure, Image, ImageFormat, Message, Part, Role, ToolResult},
};
use serde_json::{Value, json};

mod samples;

fn settings(options: Value) -> Settings {
    Settings {
        model: "configured-model".into(),
        options: options.to_string(),
    }
}

fn request(messages: Vec<Message>) -> Request {
    Request {
        messages,
        tools: vec![],
        max_output_tokens: 1024,
    }
}

/// `history` after the user message that asked for it.
fn asked(history: Vec<Message>) -> Vec<Message> {
    let question = Message {
        role: Role::User,
        parts: vec![Part::Text("Read both.".into())],
    };
    std::iter::once(question).chain(history).collect()
}

fn reply(content: Value, finish: &str) -> Value {
    json!({"candidates": [{"content": content, "finishReason": finish}]})
}

fn call(id: &str) -> Value {
    json!({"functionCall": {"id": id, "name": "read", "args": {}}})
}

fn result(id: &str, is_error: bool) -> Message {
    Message {
        role: Role::Tool,
        parts: vec![Part::ToolResult(ToolResult {
            call_id: id.into(),
            content: format!("result {id}"),
            is_error,
        })],
    }
}

#[test]
fn results_return_in_call_order_as_one_user_content() -> Result<(), Failure> {
    let assistant = response::completion(reply(
        json!({"role": "model", "parts": [call("a"), call("b")]}),
        "STOP",
    ))?;
    let (body, _) = request::completion(
        &settings(json!({})),
        request(asked(vec![
            assistant.message,
            result("b", true),
            result("a", false),
        ])),
    )?;
    assert_eq!(body["contents"].as_array().map(Vec::len), Some(3));
    let responses = &body["contents"][2]["parts"];
    assert_eq!(responses[0]["functionResponse"]["id"], "a");
    assert_eq!(responses[1]["functionResponse"]["id"], "b");
    assert_eq!(
        responses[1]["functionResponse"]["response"],
        json!({"error": "result b"})
    );
    Ok(())
}

#[test]
fn calls_without_ids_take_their_results_in_order() -> Result<(), Failure> {
    let unnamed = |name: &str| json!({"functionCall": {"name": name, "args": {}}});
    let assistant = response::completion(reply(
        json!({"role": "model", "parts": [unnamed("first"), unnamed("second")]}),
        "STOP",
    ))?;
    assert_eq!(assistant.stop_reason, StopReason::ToolCalls);
    let (body, _) = request::completion(
        &settings(json!({})),
        request(asked(vec![
            assistant.message,
            result("", false),
            result("", true),
        ])),
    )?;
    let responses = &body["contents"][2]["parts"];
    assert_eq!(responses[0]["functionResponse"]["name"], "first");
    assert_eq!(responses[1]["functionResponse"]["name"], "second");
    assert_eq!(
        responses[1]["functionResponse"]["response"],
        json!({"error": "result "})
    );
    assert!(responses[0]["functionResponse"].get("id").is_none());
    Ok(())
}

#[test]
fn null_args_read_as_empty_arguments() -> Result<(), Failure> {
    let reply = response::completion(reply(
        json!({"role": "model", "parts": [{"functionCall": {"name": "self_status", "args": null}}]}),
        "STOP",
    ))?;
    assert!(matches!(
        reply.message.parts.first(),
        Some(Part::ToolCall(call)) if call.arguments == "{}"
    ));
    assert_eq!(reply.stop_reason, StopReason::ToolCalls);
    Ok(())
}

#[test]
fn only_calls_from_other_services_carry_the_placeholder_signature() -> Result<(), Failure> {
    let own = response::completion(reply(
        json!({"role": "model", "parts": [call("a")]}),
        "STOP",
    ))?
    .message;
    let other = Message {
        role: Role::Assistant,
        parts: own
            .parts
            .iter()
            .filter(|part| !matches!(part, Part::Extension(_)))
            .cloned()
            .collect(),
    };
    let (body, _) = request::completion(
        &settings(json!({})),
        request(asked(vec![
            own,
            result("a", false),
            other,
            result("a", false),
        ])),
    )?;
    assert!(body["contents"][1]["parts"][0]["thoughtSignature"].is_null());
    assert_eq!(
        body["contents"][3]["parts"][0]["thoughtSignature"],
        "skip_thought_signature_validator"
    );
    Ok(())
}

#[test]
fn a_history_that_starts_with_a_call_gets_a_user_turn_before_it() -> Result<(), Failure> {
    let calling = response::completion(reply(
        json!({"role": "model", "parts": [call("a")]}),
        "STOP",
    ))?
    .message;
    let (body, _) = request::completion(
        &settings(json!({})),
        request(vec![calling, result("a", false)]),
    )?;
    let contents = &body["contents"];
    assert_eq!(contents[0]["role"], "user");
    assert!(contents[0]["parts"][0]["text"].is_string());
    assert!(contents[1]["parts"][0]["functionCall"].is_object());

    let speaking = Message {
        role: Role::Assistant,
        parts: vec![Part::Text("Earlier I read it.".into())],
    };
    let (body, _) = request::completion(
        &settings(json!({})),
        request(vec![speaking, asked(vec![]).remove(0)]),
    )?;
    assert_eq!(body["contents"][0]["role"], "model");
    Ok(())
}

#[test]
fn truncation_takes_precedence_over_tool_calls() -> Result<(), Failure> {
    let calls = json!({"role": "model", "parts": [call("a")]});
    let text = json!({"role": "model", "parts": [{"text": "done"}]});
    for (content, finish, expected) in [
        (&calls, "MAX_TOKENS", StopReason::MaxTokens),
        (&calls, "CONTINUATION", StopReason::MaxTokens),
        (&calls, "STOP", StopReason::ToolCalls),
        (&text, "STOP", StopReason::EndTurn),
        (&text, "SAFETY", StopReason::Other),
    ] {
        let stop = response::completion(reply(content.clone(), finish))?.stop_reason;
        assert_eq!(stop, expected, "{finish}");
    }
    Ok(())
}

#[test]
fn a_reply_without_candidates_reports_the_prompt_feedback() {
    let response = json!({"promptFeedback": {"blockReason": "PROHIBITED_CONTENT"}});
    assert!(
        response::completion(response)
            .is_err_and(|failure| failure.message.contains("PROHIBITED_CONTENT"))
    );
}

#[test]
fn thinking_that_exhausts_the_output_budget_is_an_empty_reply() -> Result<(), Failure> {
    // Gemini's answer when thinking uses up maxOutputTokens.
    let completion = response::completion(json!({
        "candidates": [{"content": {}, "finishReason": "MAX_TOKENS", "index": 0}],
        "usageMetadata": {"promptTokenCount": 8, "totalTokenCount": 108, "thoughtsTokenCount": 100}
    }))?;
    assert_eq!(completion.stop_reason, StopReason::MaxTokens);
    assert!(matches!(
        completion.message.parts.as_slice(),
        [Part::Extension(extension)] if extension.data == "{}"
    ));
    Ok(())
}

#[test]
fn user_images_are_inline_parts() -> Result<(), Failure> {
    let (body, _) = request::completion(
        &settings(json!({})),
        request(vec![Message {
            role: Role::User,
            parts: vec![
                Part::Image(Image {
                    format: ImageFormat::Png,
                    data: vec![0],
                }),
                Part::Text("describe".into()),
            ],
        }]),
    )?;
    assert_eq!(
        body["contents"][0]["parts"],
        json!([{"inlineData": {"mimeType": "image/png", "data": "AA=="}}, {"text": "describe"}])
    );
    Ok(())
}

#[test]
fn options_cannot_override_safety_settings() {
    let options = json!({"safetySettings": [
        {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_LOW_AND_ABOVE"}
    ]});
    assert!(
        request::completion(&settings(options), request(vec![]))
            .is_err_and(|failure| failure.message == "options cannot override safetySettings")
    );
}

/// What `decode` makes of a successful streamed `body`.
fn streamed(body: &str) -> Result<Completion, CompleteError> {
    Plugin::decode(HttpResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        body: body.as_bytes().to_vec(),
    })
}

/// The model turn a reply keeps whole for replay.
fn kept(reply: &Completion) -> Value {
    reply
        .message
        .parts
        .iter()
        .find_map(|part| match part {
            Part::Extension(extension) => serde_json::from_str(&extension.data).ok(),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn requests_stream_unless_the_owner_turns_it_off() -> Result<(), Failure> {
    let streamed = Plugin::encode(settings(json!({})), request(vec![]))?;
    assert_eq!(
        streamed.url,
        "/models/configured-model:streamGenerateContent?alt=sse"
    );
    assert!(streamed.idle_timeout_ms.is_some());
    let whole = Plugin::encode(settings(json!({"stream": false})), request(vec![]))?;
    assert_eq!(whole.url, "/models/configured-model:generateContent");
    assert!(whole.idle_timeout_ms.is_none());
    let body: Value = serde_json::from_slice(whole.body.as_deref().unwrap_or_default())
        .map_err(|e| enco_sdk::protocol::bad(e.to_string()))?;
    assert!(body.get("stream").is_none());
    Ok(())
}

#[test]
fn a_stream_keeps_its_signed_parts_where_they_came() -> Result<(), CompleteError> {
    let call = streamed(samples::SIGNED_CALL)?;
    assert_eq!(call.stop_reason, StopReason::ToolCalls);
    assert_eq!(call.usage.input_tokens, 68);
    assert!(matches!(
        call.message.parts.as_slice(),
        [Part::ToolCall(called), Part::Extension(_)]
            if called.id == "call_102324" && called.arguments == r#"{"city":"Nice"}"#
    ));
    let parts = &kept(&call)["parts"];
    assert_eq!(parts.as_array().map(Vec::len), Some(1));
    assert!(parts[0]["thoughtSignature"].is_string());
    let text = streamed(samples::TEXT)?;
    assert_eq!(text.stop_reason, StopReason::EndTurn);
    assert!(matches!(
        text.message.parts.as_slice(),
        [Part::Text(said), Part::Extension(_)] if said == "Hello, how are you today?"
    ));
    let parts = &kept(&text)["parts"];
    assert_eq!(parts[0], json!({"text": "Hello, how are you today?"}));
    assert_eq!(parts[1]["text"], "");
    assert!(parts[1]["thoughtSignature"].is_string());
    Ok(())
}

#[test]
fn a_stream_that_stops_early_or_reports_an_error_may_be_retried() {
    let failure = |body: &str| match streamed(body) {
        Err(CompleteError::Failed(failure)) => Some(failure),
        _ => None,
    };
    let unfinished: String = samples::TEXT
        .split("\n\n")
        .filter(|event| !event.contains("finishReason"))
        .collect::<Vec<_>>()
        .join("\n\n");
    assert!(failure(&unfinished).is_some_and(|f| f.code == "provider.server" && f.retryable));
    let busy = "data: {\"error\": {\"code\": 503, \"message\": \"busy\", \"status\": \"UNAVAILABLE\"}}\n\n";
    assert!(failure(busy).is_some_and(|f| f.code == "provider.server" && f.retryable));
}
