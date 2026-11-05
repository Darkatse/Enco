use super::{Plugin, completion::Guest as _};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpResponse, Request, Settings, StopReason, bad,
    types::{
        Effect, Failure, Image, ImageFormat, Message, Part, Role, ToolCall, ToolResult, ToolSpec,
    },
};
use serde_json::{Value, json};

mod samples;
use samples::{
    COMMENTARY_REPLY, INCOMPLETE_REPLY, INCOMPLETE_STREAM, REASONING_REPLY, REASONING_STREAM,
};

fn settings(options: Value) -> Settings {
    Settings {
        model: "gpt-5.4-mini".into(),
        options: options.to_string(),
    }
}

/// The JSON body `encode` gives for `request`.
fn encode_request(request: Request, options: Value) -> Result<Value, Failure> {
    let body = Plugin::encode(settings(options), request)?
        .body
        .ok_or_else(|| bad("a request without a body"))?;
    serde_json::from_slice(&body).map_err(|error| bad(error.to_string()))
}

/// The JSON body `encode` gives for `messages`, without tools.
fn encode(messages: Vec<Message>, options: Value) -> Result<Value, Failure> {
    let request = Request {
        messages,
        tools: vec![],
        max_output_tokens: 2000,
    };
    encode_request(request, options)
}

fn decode(status: u16, body: &Value) -> Result<Completion, CompleteError> {
    Plugin::decode(HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.to_string().into_bytes(),
    })
}

fn completed(body: &Value) -> Result<Completion, Failure> {
    decode(200, body).map_err(|error| match error {
        CompleteError::Failed(failure) => failure,
        CompleteError::Overflow(overflow) => bad(overflow.message),
    })
}

fn sample(text: &str) -> Result<Value, Failure> {
    serde_json::from_str(text).map_err(|error| bad(error.to_string()))
}

/// The refusal envelope the service sends with HTTP 400.
fn refusal(message: &str, param: &str, code: &str) -> Value {
    json!({
        "error": {
            "message": message,
            "type": "invalid_request_error",
            "param": param,
            "code": code
        }
    })
}

fn said(role: Role, text: &str) -> Message {
    Message {
        role,
        parts: vec![Part::Text(text.into())],
    }
}

fn image() -> Part {
    Part::Image(Image {
        format: ImageFormat::Png,
        data: vec![1, 2, 3],
    })
}

fn calling(calls: &[(&str, &str)]) -> Message {
    let calls = calls.iter().map(|(id, city)| {
        Part::ToolCall(ToolCall {
            id: (*id).into(),
            name: "get_weather".into(),
            arguments: json!({"city": city}).to_string(),
        })
    });
    Message {
        role: Role::Assistant,
        parts: std::iter::once(Part::Text("Checking.".into()))
            .chain(calls)
            .collect(),
    }
}

fn result(id: &str, content: &str, images: Vec<Part>) -> Message {
    let result = Part::ToolResult(ToolResult {
        call_id: id.into(),
        content: content.into(),
        is_error: false,
    });
    Message {
        role: Role::Tool,
        parts: std::iter::once(result).chain(images).collect(),
    }
}

#[test]
fn messages_become_items_in_place_and_nothing_stays_on_the_service() -> Result<(), Failure> {
    let question = Message {
        role: Role::User,
        parts: vec![Part::Text("Which is warmer?".into()), image()],
    };
    let body = encode(
        vec![
            said(Role::System, "Answer tersely."),
            said(Role::Assistant, "Paris is 12°C."),
            said(Role::System, "Earlier, the owner asked about Paris."),
            question,
        ],
        json!({}),
    )?;
    assert_eq!(body["store"], json!(false));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["max_output_tokens"], 2000);
    let text = |text: &str| json!([{"type": "input_text", "text": text}]);
    assert_eq!(
        body["input"],
        json!([
            {"type": "message", "role": "system", "content": text("Answer tersely.")},
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Paris is 12°C."}]
            },
            {
                "type": "message",
                "role": "system",
                "content": text("Earlier, the owner asked about Paris.")
            },
            {
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Which is warmer?"},
                    {"type": "input_image", "image_url": "data:image/png;base64,AQID"}
                ]
            }
        ])
    );
    Ok(())
}

#[test]
fn a_conversation_keeps_its_prompt_cache_key_as_it_grows() -> Result<(), Failure> {
    let key = |messages| encode(messages, json!({})).map(|body| body["prompt_cache_key"].clone());
    let opening = vec![
        said(Role::System, "Answer tersely."),
        said(Role::User, "Which is warmer?"),
    ];
    let mut later = opening.clone();
    later.extend([
        said(Role::Assistant, "Paris."),
        said(Role::User, "And tomorrow?"),
    ]);
    let another = vec![
        said(Role::System, "Answer tersely."),
        said(Role::User, "Which is colder?"),
    ];
    let first = key(opening)?;
    assert!(first.is_string());
    assert_eq!(key(later)?, first);
    assert_ne!(key(another)?, first);
    Ok(())
}

#[test]
fn tools_are_flat_functions_with_strict_off() -> Result<(), Failure> {
    let schema = json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"],
        "additionalProperties": false
    });
    let request = Request {
        messages: vec![said(Role::User, "Weather in Nice?")],
        tools: vec![ToolSpec {
            name: "get_weather".into(),
            description: "Current weather for a city.".into(),
            input_schema: schema.to_string(),
            effect: Effect::ReadOnly,
        }],
        max_output_tokens: 2000,
    };
    let body = encode_request(request, json!({}))?;
    assert_eq!(
        body["tools"],
        json!([{
            "type": "function",
            "name": "get_weather",
            "description": "Current weather for a city.",
            "parameters": schema,
            "strict": false
        }])
    );
    Ok(())
}

#[test]
fn tool_outputs_follow_their_calls_in_call_order() -> Result<(), Failure> {
    let body = encode(
        vec![
            said(Role::User, "Paris or Nice?"),
            calling(&[("call_a", "Paris"), ("call_b", "Nice")]),
            result("call_b", "17°C", vec![image()]),
            result("call_a", "12°C", vec![]),
            said(Role::User, "Thanks."),
        ],
        json!({}),
    )?;
    let input = &body["input"];
    assert_eq!(input.as_array().map(Vec::len), Some(7));
    assert_eq!(
        input[2],
        json!({
            "type": "function_call",
            "call_id": "call_a",
            "name": "get_weather",
            "arguments": r#"{"city":"Paris"}"#
        })
    );
    assert_eq!(input[3]["call_id"], "call_b");
    assert_eq!(
        input[4],
        json!({"type": "function_call_output", "call_id": "call_a", "output": "12°C"})
    );
    assert_eq!(
        input[5],
        json!({
            "type": "function_call_output",
            "call_id": "call_b",
            "output": [
                {"type": "input_text", "text": "17°C"},
                {"type": "input_image", "image_url": "data:image/png;base64,AQID"}
            ]
        })
    );
    assert_eq!(input[6]["content"][0]["text"], "Thanks.");
    Ok(())
}

#[test]
fn calls_without_ids_share_a_synthesized_id_with_their_outputs() -> Result<(), Failure> {
    let body = encode(
        vec![
            said(Role::User, "Paris or Nice?"),
            calling(&[("", "Paris"), ("", "Nice")]),
            result("", "12°C", vec![]),
            result("", "17°C", vec![]),
        ],
        json!({}),
    )?;
    let input = &body["input"];
    let (first, second) = (&input[2]["call_id"], &input[3]["call_id"]);
    assert!(first.as_str().is_some_and(|id| !id.is_empty()));
    assert!(second.as_str().is_some_and(|id| !id.is_empty()));
    assert_ne!(first, second);
    assert_eq!(&input[4]["call_id"], first);
    assert_eq!(input[4]["output"], "12°C");
    assert_eq!(&input[5]["call_id"], second);
    assert_eq!(input[5]["output"], "17°C");
    Ok(())
}

#[test]
fn options_cannot_keep_state_on_the_service_or_truncate() {
    let previous = "resp_02730356b09e1e79016aeb9b89b60887d29373bad8bbd48665";
    for options in [
        json!({"previous_response_id": previous}),
        json!({"truncation": "auto"}),
    ] {
        let encoded = encode(vec![said(Role::User, "Hi.")], options);
        assert!(encoded.is_err_and(|failure| failure.code == "provider.bad_request"));
    }
}

#[test]
fn a_reasoning_reply_goes_back_exactly_as_it_came() -> Result<(), Failure> {
    let sample = sample(REASONING_REPLY)?;
    let reply = completed(&sample)?;
    assert_eq!(reply.stop_reason, StopReason::ToolCalls);
    let [Part::ToolCall(call), Part::Extension(_)] = reply.message.parts.as_slice() else {
        return Err(bad(format!("unexpected parts {:?}", reply.message.parts)));
    };
    assert_eq!(call.id, "call_BoUa9IP2QQWlhrI0mZn6jNSe");
    assert_eq!(call.arguments, r#"{"city":"Nice"}"#);
    let id = call.id.clone();
    let body = encode(
        vec![
            said(
                Role::User,
                "Which is warmest in November: Paris, Lyon or Nice?",
            ),
            reply.message,
            result(&id, "17°C", vec![]),
        ],
        json!({}),
    )?;
    let input = &body["input"];
    assert_eq!(input.as_array().map(Vec::len), Some(4));
    assert_eq!(input[1], sample["output"][0]);
    assert_eq!(input[2], sample["output"][1]);
    assert_eq!(
        input[3],
        json!({"type": "function_call_output", "call_id": id, "output": "17°C"})
    );
    Ok(())
}

#[test]
fn a_commentary_reply_yields_its_text_and_call() -> Result<(), Failure> {
    let reply = completed(&sample(COMMENTARY_REPLY)?)?;
    assert_eq!(reply.stop_reason, StopReason::ToolCalls);
    let [Part::Text(text), Part::ToolCall(call), Part::Extension(_)] =
        reply.message.parts.as_slice()
    else {
        return Err(bad(format!("unexpected parts {:?}", reply.message.parts)));
    };
    assert_eq!(
        text,
        "Nice is most likely warmest in November, so I\u{2019}ll check its weather."
    );
    assert_eq!(call.id, "call_Sh0QSf0QoVQlHB5Rvmjz4lX0");
    assert_eq!(call.name, "get_weather");
    assert_eq!(reply.usage.input_tokens, 89);
    assert_eq!(reply.usage.output_tokens, 41);
    assert_eq!(reply.usage.cached_input_tokens, Some(0));
    Ok(())
}

#[test]
fn an_incomplete_reply_at_the_output_limit_is_max_tokens() -> Result<(), Failure> {
    let reply = completed(&sample(INCOMPLETE_REPLY)?)?;
    assert_eq!(reply.stop_reason, StopReason::MaxTokens);
    assert!(matches!(
        reply.message.parts.as_slice(),
        [Part::Extension(_)]
    ));
    Ok(())
}

#[test]
fn another_refusal_is_a_bad_request() {
    let answer = decode(
        400,
        &refusal(
            "Invalid 'max_output_tokens': integer below minimum value. Expected a value >= 16, but got 1 instead.",
            "max_output_tokens",
            "integer_below_min_value",
        ),
    );
    assert!(matches!(
        answer,
        Err(CompleteError::Failed(failure)) if failure.code == "provider.bad_request"
    ));
}

/// What `decode` makes of a successful streamed `body`.
fn streamed(body: &str) -> Result<Completion, CompleteError> {
    Plugin::decode(HttpResponse {
        status: 200,
        headers: vec![(
            "content-type".into(),
            "text/event-stream; charset=utf-8".into(),
        )],
        body: body.as_bytes().to_vec(),
    })
}

#[test]
fn requests_stream_unless_the_owner_turns_it_off() -> Result<(), Failure> {
    let request = || Request {
        messages: vec![said(Role::User, "Hi.")],
        tools: vec![],
        max_output_tokens: 2000,
    };
    let streamed = Plugin::encode(settings(json!({})), request())?;
    assert!(streamed.idle_timeout_ms.is_some());
    assert_eq!(
        encode(vec![said(Role::User, "Hi.")], json!({}))?["stream"],
        true
    );
    let whole = Plugin::encode(settings(json!({"stream": false})), request())?;
    assert!(whole.idle_timeout_ms.is_none());
    let body = encode(vec![said(Role::User, "Hi.")], json!({"stream": false}))?;
    assert!(body.get("stream").is_none());
    Ok(())
}

#[test]
fn a_stream_reads_as_the_response_it_ends_with() -> Result<(), CompleteError> {
    let reply = streamed(REASONING_STREAM)?;
    assert_eq!(reply.stop_reason, StopReason::ToolCalls);
    assert_eq!(reply.usage.input_tokens, 72);
    assert_eq!(reply.usage.output_tokens, 175);
    let cities: Vec<_> = reply
        .message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall(call) => Some(call.arguments.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        cities,
        [
            r#"{"city":"Paris"}"#,
            r#"{"city":"Lyon"}"#,
            r#"{"city":"Nice"}"#
        ]
    );
    assert!(matches!(
        reply.message.parts.last(),
        Some(Part::Extension(_))
    ));
    let cut = streamed(INCOMPLETE_STREAM)?;
    assert_eq!(cut.stop_reason, StopReason::MaxTokens);
    Ok(())
}

#[test]
fn a_stream_without_its_end_or_with_a_failure_fails_by_code() {
    let failure = |body: &str| match streamed(body) {
        Err(CompleteError::Failed(failure)) => Some(failure),
        _ => None,
    };
    let unfinished: String = REASONING_STREAM
        .split("\n\n")
        .filter(|event| !event.contains("response.completed"))
        .collect::<Vec<_>>()
        .join("\n\n");
    assert!(failure(&unfinished).is_some_and(|f| f.code == "provider.server" && f.retryable));
    let failed = |code: &str| {
        format!(
            "event: response.failed\ndata: {{\"type\":\"response.failed\",\"response\":{{\"status\":\"failed\",\"error\":{{\"code\":\"{code}\",\"message\":\"m\"}}}}}}\n\n"
        )
    };
    assert!(failure(&failed("server_error")).is_some_and(|f| f.retryable));
    assert!(
        failure(&failed("invalid_prompt"))
            .is_some_and(|f| f.code == "provider.bad_response" && !f.retryable)
    );
    assert!(matches!(
        streamed(&failed("context_length_exceeded")),
        Err(CompleteError::Overflow(_))
    ));
    let error = "event: error\ndata: {\"type\":\"error\",\"code\":\"rate_limit_exceeded\",\"message\":\"slow down\"}\n\n";
    assert!(failure(error).is_some_and(|f| f.code == "provider.rate_limited" && f.retryable));
}
