use super::{request, response};
use enco_sdk::protocol::{
    Request, Settings, StopReason,
    types::{Failure, Image, ImageFormat, Message, Part, Role, ToolResult},
};
use serde_json::{Value, json};

fn settings(options: Value) -> Settings {
    Settings {
        base_url: "https://example.test/v1beta".into(),
        model: "configured-model".into(),
        options: options.to_string(),
    }
}

fn request(messages: Vec<Message>) -> Request {
    Request {
        messages,
        tools: vec![],
        max_output_tokens: None,
    }
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
    let body = request::completion(
        &settings(json!({})),
        request(vec![
            assistant.message,
            result("b", true),
            result("a", false),
        ]),
    )?;
    assert_eq!(body["contents"].as_array().map(Vec::len), Some(2));
    let responses = &body["contents"][1]["parts"];
    assert_eq!(responses[0]["functionResponse"]["id"], "a");
    assert_eq!(responses[1]["functionResponse"]["id"], "b");
    assert_eq!(
        responses[1]["functionResponse"]["response"],
        json!({"error": "result b"})
    );
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
    let body = request::completion(
        &settings(json!({})),
        request(vec![own, result("a", false), other, result("a", false)]),
    )?;
    assert!(body["contents"][0]["parts"][0]["thoughtSignature"].is_null());
    assert_eq!(
        body["contents"][2]["parts"][0]["thoughtSignature"],
        "skip_thought_signature_validator"
    );
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
    let body = request::completion(
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
