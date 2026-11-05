use super::{Plugin, completion::Guest as _};
use enco_sdk::protocol::{
    CompleteError, Completion, HttpResponse, Request, Settings, StopReason, bad,
    types::{Failure, Image, ImageFormat, Message, Part, Role, ToolCall, ToolResult},
};
use serde_json::{Value, json};

mod samples;

/// A real reply from `claude-haiku-5-5` with adaptive thinking at high effort.
const THINKING_REPLY: &str = r#"{
  "model": "claude-haiku-5-5",
  "id": "msg_011CgiYuVBTQ6gB5HDWxY4BN",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "thinking",
      "thinking": "I'm weighing which city will be warmest in November, and Nice stands out with its Mediterranean climate and mild winters, with average highs near 17°C. Paris and Lyon both sit around 12-13°C with more fog and continental chill, so Nice looks like the clear pick. I'll call the tool for Nice and keep my reasoning brief, since the system asks for terse answers.\n\nI'm settling on a short rationale to share with the user before calling the tool, keeping it brief since the system asks for terse answers.\n\n",
      "signature": "CAQS/wgKEAgTGAI4AUIIdGhpbmtpbmcSDHz6MDD+MNvslRLhWRoMh9M1emCehDgpRZRbIjDdUGEsVbWiitgOCD/IDGI5gQuWme1dvGVkKxGdRQy8ni2DGhTPjuYPwHuzoYayMmQqnAhsAMUspPiX8kRKXzVDv29VCVNupmrAGhLqEv7vJuFJcCBYkbJC6gQyTygxVpnOtoUxLerpxM0YQd7t4qd7WioykJHO6+ofBzswjv1VxkKjbbsI+JOasCJcdiqx1W5xzx5Ps+RtKBCh5RszGuetQwEuHTobo2BVhXiMPD7RqghoMZxrJY5l6ALriVYuBS9E2gtN/1E90Njk3kz3oI6lX70boIPD+HVx+07CrqqNktbxMhJmTz0HcnftJVAH0wHyDPZea9jKqH0TY0UzMcev8k7wJiKXcxIwN/6nqlVX7ofsjX1H3jKjMo1hI4/Tvgd+nZhcGbDq4pNWFr2SLl9KFN4uJ+y1+zXQGpR/7/S+irvaQyeDt1BGMquBbgE8D5wuD0EXfwH1DkGgB+XIGyVqxIhfG134wmILKcf+cAyllh18sYJB1NqtAgutGlbcebJ8MuIczl9RaXxfVw48eDCpMhaxhICPGY+Wj1k7ldJxEGIfIUUL60ZyNMMDKjBTXoVyG5GObCUap2PT46TakgOrI9NFuub9qEuFQI+L7F54oGqe27jYsuUQRZYiCsmx0F2Itj+4rCe4Gu+JYXvX8k+US7JMF05gHCKITYhEtag/xzgkwhIW+ogWy53a42OjHBkt1IJpB5ytPjEHFyHXgtR7SBMbk1pOkUgtRN8yrXw0GLN/widl+orENG4W3XCo1feb6VmVFFTDsMzTUCaq9O7SL88c3/1B28mqWIJ8X8RN8o9iywxa+qcUDJN7TrdmqGYk7jLlRSvdBktROAQbqIy3H/IPg6T/sQw5lNvKwN7r6tiGrQIzodUsASVo3rCkMHYz/j66IItno6mqjvXu3pPZg/vTJkS2wgkGRIgb27FPjk6f415b2fS2goU33fDxmCYVBWltobD7soLYlGdt+HealMlVkS08R2elN6iqhQv5qfJvOaAtHdxocoMi6onW6AbbcYCfx3/l884COQLqa9ZjD128XCLucisKHeLCkLfsffDwTZeEmwxVhnAcJ0y79y4+19T34cEtF5O2ILPuoU1y4wLKvtwmwbEevT6Tf7SAD++6OgOleXFaftbvraRvJFWmjXRDnaadCkB1q4/j5bCzhWMOBmNnh49vd97BegccptFKLqpiQxEFyy7mycs6ySO4T3vt/DUvsnHcxE1FPjH3ktk/Ttud04NRoakaqDuscD2sOu1qcdInZqhMoiVDv2btIGl9yqOSbfgy8JIkh+IshVAXnQqfp20hxMPdzR127WAAt4VWz0cU9XTXrG3419F7Z0703LWz2Xf65pLHqDt6C833abgWDyNaAFbo7T8ZdOM7mVuOF+j581mWUZ0OuDpfSkORZEEm9wWlxQJPxPou0N5rJBQKFQKIwL5tZkWwIUJkOQXBXzZ1WIi5KCeGwxgB"
    },
    {
      "type": "text",
      "text": "Nice is likely the warmest in November. It sits on the Mediterranean coast, where the sea holds heat and moderates temperatures, and it's sheltered from cold northern air by the Alps. Paris and Lyon are inland with a more continental climate, so they run cooler, and Paris is also damp and often overcast."
    },
    {
      "type": "tool_use",
      "id": "toolu_01GaLb5BhVp5697SD5hSFwLd",
      "name": "get_weather",
      "input": {"city": "Nice"},
      "caller": {"type": "direct"}
    }
  ],
  "container": null,
  "stop_reason": "tool_use",
  "stop_sequence": null,
  "stop_details": null,
  "usage": {
    "input_tokens": 445,
    "cache_creation_input_tokens": 0,
    "cache_read_input_tokens": 0,
    "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 0},
    "output_tokens": 345,
    "output_tokens_details": {"thinking_tokens": 191},
    "service_tier": "standard",
    "inference_geo": "global",
    "speed": "standard"
  },
  "diagnostics": null
}"#;

fn settings(options: Value) -> Settings {
    Settings {
        model: "claude-haiku-5-5".into(),
        options: options.to_string(),
    }
}

/// The JSON body `encode` gives for `messages`.
fn encode(messages: Vec<Message>, options: Value) -> Result<Value, Failure> {
    let request = Request {
        messages,
        tools: vec![],
        max_output_tokens: 1024,
    };
    let body = Plugin::encode(settings(options), request)?
        .body
        .ok_or_else(|| bad("a request without a body"))?;
    serde_json::from_slice(&body).map_err(|error| bad(error.to_string()))
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

fn thinking_reply() -> Result<Value, Failure> {
    serde_json::from_str(THINKING_REPLY).map_err(|error| bad(error.to_string()))
}

/// A real reply's envelope around a text-only answer with these usage counts.
fn text_reply(input: u64, created: u64, read: u64) -> Result<Value, Failure> {
    let mut reply = thinking_reply()?;
    reply["content"] = json!([reply["content"][1].clone()]);
    reply["stop_reason"] = json!("end_turn");
    reply["usage"] = json!({
        "input_tokens": input,
        "cache_creation_input_tokens": created,
        "cache_read_input_tokens": read,
        "cache_creation": {"ephemeral_5m_input_tokens": created, "ephemeral_1h_input_tokens": 0},
        "output_tokens": 62,
        "output_tokens_details": {"thinking_tokens": 0},
        "service_tier": "standard",
        "inference_geo": "global",
        "speed": "standard"
    });
    Ok(reply)
}

/// The refusal envelope the service sends with HTTP 400.
fn refusal(message: &str) -> Value {
    json!({
        "type": "error",
        "error": {"type": "invalid_request_error", "message": message},
        "request_id": "req_011CgiZ1eFaHSvko7AhQdyvD"
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
fn system_text_and_the_cache_marker_go_top_level() -> Result<(), Failure> {
    let question = Message {
        role: Role::User,
        parts: vec![Part::Text("Which is warmer?".into()), image()],
    };
    let body = encode(
        vec![
            said(Role::System, "Answer tersely."),
            said(Role::System, "Earlier, the owner asked about Paris."),
            said(Role::Assistant, "Paris is 12°C."),
            question,
        ],
        json!({}),
    )?;
    assert_eq!(
        body["system"],
        json!([
            {"type": "text", "text": "Answer tersely."},
            {"type": "text", "text": "Earlier, the owner asked about Paris."}
        ])
    );
    assert_eq!(body["cache_control"], json!({"type": "ephemeral"}));
    // The service accepts a history that starts with the assistant as it is.
    assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["messages"][0]["role"], "assistant");
    let image = json!({"type": "base64", "media_type": "image/png", "data": "AQID"});
    assert_eq!(
        body["messages"][1]["content"],
        json!([
            {"type": "text", "text": "Which is warmer?"},
            {"type": "image", "source": image}
        ])
    );
    Ok(())
}

#[test]
fn tool_results_return_in_call_order_as_one_user_message() -> Result<(), Failure> {
    let body = encode(
        vec![
            said(Role::User, "Paris or Nice?"),
            calling(&[("toolu_a", "Paris"), ("toolu_b", "Nice")]),
            result("toolu_b", "17°C", vec![image()]),
            result("toolu_a", "12°C", vec![]),
            said(Role::User, "Thanks."),
        ],
        json!({}),
    )?;
    let messages = &body["messages"];
    assert_eq!(messages.as_array().map(Vec::len), Some(4));
    assert_eq!(messages[1]["content"][1]["input"], json!({"city": "Paris"}));
    assert_eq!(messages[2]["role"], "user");
    let results = &messages[2]["content"];
    assert_eq!(results.as_array().map(Vec::len), Some(2));
    assert_eq!(results[0]["tool_use_id"], "toolu_a");
    assert_eq!(
        results[0]["content"],
        json!([{"type": "text", "text": "12°C"}])
    );
    assert_eq!(results[1]["tool_use_id"], "toolu_b");
    assert_eq!(results[1]["content"][0]["text"], "17°C");
    assert_eq!(results[1]["content"][1]["type"], "image");
    assert_eq!(messages[3]["content"][0]["text"], "Thanks.");
    Ok(())
}

#[test]
fn calls_without_ids_share_a_synthesized_id_with_their_results() -> Result<(), Failure> {
    let body = encode(
        vec![
            said(Role::User, "Paris or Nice?"),
            calling(&[("", "Paris"), ("", "Nice")]),
            result("", "12°C", vec![]),
            result("", "17°C", vec![]),
        ],
        json!({}),
    )?;
    let calls = &body["messages"][1]["content"];
    let results = &body["messages"][2]["content"];
    let (first, second) = (&calls[1]["id"], &calls[2]["id"]);
    assert!(first.as_str().is_some_and(|id| !id.is_empty()));
    assert!(second.as_str().is_some_and(|id| !id.is_empty()));
    assert_ne!(first, second);
    assert_eq!(&results[0]["tool_use_id"], first);
    assert_eq!(results[0]["content"][0]["text"], "12°C");
    assert_eq!(&results[1]["tool_use_id"], second);
    assert_eq!(results[1]["content"][0]["text"], "17°C");
    Ok(())
}

#[test]
fn options_cannot_override_reserved_fields() {
    for options in [json!({"max_tokens": 64}), json!({"stream": "on"})] {
        let encoded = encode(vec![said(Role::User, "Hi.")], options);
        assert!(encoded.is_err_and(|failure| failure.code == "provider.bad_request"));
    }
}

#[test]
fn a_thinking_reply_goes_back_exactly_as_it_came() -> Result<(), Failure> {
    let sample = thinking_reply()?;
    let reply = completed(&sample)?;
    assert_eq!(reply.stop_reason, StopReason::ToolCalls);
    let [Part::Text(_), Part::ToolCall(call), Part::Extension(_)] = reply.message.parts.as_slice()
    else {
        return Err(bad(format!("unexpected parts {:?}", reply.message.parts)));
    };
    assert_eq!(call.id, "toolu_01GaLb5BhVp5697SD5hSFwLd");
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
    assert_eq!(
        body["messages"][1],
        json!({"role": "assistant", "content": sample["content"]})
    );
    assert_eq!(
        body["messages"][2]["content"][0]["tool_use_id"],
        id.as_str()
    );
    Ok(())
}

#[test]
fn a_text_reply_keeps_no_extension() -> Result<(), Failure> {
    let sample = text_reply(445, 0, 0)?;
    let reply = completed(&sample)?;
    assert_eq!(reply.stop_reason, StopReason::EndTurn);
    assert!(matches!(reply.message.parts.as_slice(), [Part::Text(_)]));
    let body = encode(
        vec![said(Role::User, "Which is warmest?"), reply.message],
        json!({}),
    )?;
    assert_eq!(body["messages"][1]["content"], sample["content"]);
    Ok(())
}

#[test]
fn input_usage_adds_what_the_cache_wrote_and_served() -> Result<(), Failure> {
    let written = completed(&text_reply(4, 8022, 0)?)?.usage;
    assert_eq!(written.input_tokens, 8026);
    assert_eq!(written.cached_input_tokens, Some(0));
    let served = completed(&text_reply(4, 0, 8022)?)?.usage;
    assert_eq!(served.input_tokens, 8026);
    assert_eq!(served.cached_input_tokens, Some(8022));
    Ok(())
}

#[test]
fn another_refusal_is_a_bad_request() {
    let answer = decode(400, &refusal("thinking.type: Field required"));
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

fn stream_failure(body: &str) -> Result<Failure, Failure> {
    match streamed(body) {
        Err(CompleteError::Failed(failure)) => Ok(failure),
        other => Err(bad(format!("expected a failure, got {other:?}"))),
    }
}

#[test]
fn requests_stream_unless_the_owner_turns_it_off() -> Result<(), Failure> {
    let request = || Request {
        messages: vec![said(Role::User, "Hi.")],
        tools: vec![],
        max_output_tokens: 1024,
    };
    let streamed = Plugin::encode(settings(json!({})), request())?;
    assert!(streamed.idle_timeout_ms.is_some());
    assert!(
        streamed
            .headers
            .contains(&("accept".into(), "text/event-stream".into()))
    );
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
fn a_stream_reads_as_the_message_it_spells() -> Result<(), CompleteError> {
    let reply = streamed(samples::THINKING_CALL)?;
    assert_eq!(reply.stop_reason, StopReason::ToolCalls);
    assert_eq!(reply.usage.input_tokens, 406);
    assert_eq!(reply.usage.output_tokens, 331);
    let [Part::ToolCall(call), Part::Extension(whole)] = reply.message.parts.as_slice() else {
        return Err(CompleteError::Failed(bad(format!(
            "unexpected parts {:?}",
            reply.message.parts
        ))));
    };
    assert_eq!(call.id, "toolu_017GSxXn5j52bDCWB5mTXMoc");
    assert_eq!(call.arguments, r#"{"city":"Nice"}"#);
    let whole: Value =
        serde_json::from_str(&whole.data).map_err(|e| CompleteError::Failed(bad(e.to_string())))?;
    let thinking = &whole["content"][0];
    assert_eq!(thinking["type"], "thinking");
    assert!(
        thinking["signature"]
            .as_str()
            .is_some_and(|signature| signature.starts_with("CAQSsgsK"))
    );
    assert_eq!(whole["content"][1]["input"], json!({"city": "Nice"}));
    let text = streamed(samples::TEXT)?;
    assert_eq!(text.stop_reason, StopReason::EndTurn);
    let [Part::Text(said)] = text.message.parts.as_slice() else {
        return Err(CompleteError::Failed(bad(format!(
            "unexpected parts {:?}",
            text.message.parts
        ))));
    };
    assert!(said.starts_with("Hello there, friend! How are you?"));
    assert!(said.ends_with("Hello there, friend, good day!"));
    Ok(())
}

#[test]
fn a_call_that_never_stops_keeps_its_input_as_unstreamed_replies_do() -> Result<(), CompleteError> {
    // The same request unstreamed answered `"input": {}` with `max_tokens`.
    let cut = streamed(samples::CUT_CALL)?;
    assert_eq!(cut.stop_reason, StopReason::MaxTokens);
    assert!(matches!(
        cut.message.parts.as_slice(),
        [Part::ToolCall(call), Part::Extension(_)] if call.name == "write_note" && call.arguments == "{}"
    ));
    let none = streamed(samples::NO_ARGUMENTS)?;
    assert!(matches!(
        none.message.parts.as_slice(),
        [Part::ToolCall(call), Part::Extension(_)] if call.name == "get_time" && call.arguments == "{}"
    ));
    Ok(())
}

#[test]
fn a_stream_cut_short_or_overloaded_may_be_retried() -> Result<(), Failure> {
    let unfinished = samples::THINKING_CALL
        .replace("event: message_stop", "event: dropped")
        .replace("\"message_stop\"", "\"dropped\"");
    let cut = stream_failure(&unfinished)?;
    assert_eq!(cut.code, "provider.server");
    assert!(cut.retryable);
    let start = samples::TEXT.split("\n\n").next().unwrap_or_default();
    let overloaded = stream_failure(&format!(
        "{start}\n\nevent: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}}}\n\n"
    ))?;
    assert_eq!(overloaded.code, "provider.server");
    assert!(overloaded.retryable);
    let unknown = stream_failure(
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"new_error\"}}\n\n",
    )?;
    assert_eq!(unknown.code, "provider.bad_response");
    Ok(())
}
