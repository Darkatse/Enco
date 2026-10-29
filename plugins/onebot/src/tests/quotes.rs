use super::*;

/// A merged-forward node from `get_forward_msg`, which carries no message ID of its own.
fn forward_node(user: u64, card: &str, segments: Value) -> Value {
    json!({"user_id":user,"sender":{"nickname":"Nickname","card":card},"time":1793292000,
        "message_id":0,"group_id":20001,"message_type":"group","message":segments})
}

/// The data of `get_msg`: a stored group message event.
fn stored_message(user: u64, segments: Value) -> Value {
    json!({"self_id":10001,"user_id":user,"time":1793291000,"message_id":-42,"real_id":-42,
        "message_type":"group","sub_type":"normal","group_id":20001,
        "sender":{"user_id":user,"nickname":"Nickname","card":"","role":"member"},
        "message":segments,"raw_message":"","font":0,"post_type":"message"})
}

fn api(data: Value) -> Response {
    response(&json!({"status":"ok","retcode":0,"data":data}).to_string())
}

fn forward(id: &str) -> Value {
    json!([{"type":"forward","data":{"id":id}}])
}

fn text(text: &str) -> Value {
    json!([{"type":"text","data":{"text":text}}])
}

fn request_body(step: FetchStep) -> Result<(String, Value), String> {
    let FetchStep::Request(request) = step else {
        return Err("expected a request".into());
    };
    let body = serde_json::from_slice(&request.body.ok_or("body")?).map_err(|e| e.to_string())?;
    Ok((request.url, body))
}

fn quote_locator(segments: Value, direct: bool) -> Result<String, String> {
    let reading = received(frame(segments, direct))?;
    let Action::Message(content) = &reading.messages[0].action else {
        return Err("message".into());
    };
    let Some(enco_sdk::channel::Quote {
        messages: enco_sdk::channel::Quoted::Fetch(locator),
        ..
    }) = &content.quote
    else {
        return Err("quote locator".into());
    };
    Ok(locator.clone())
}

#[test]
fn reply_with_only_bot_mention_fetches_text_and_image() -> Result<(), String> {
    let locator = quote_locator(
        json!([
            {"type":"reply","data":{"id":"-42"}},
            {"type":"at","data":{"qq":"10001"}},
            {"type":"text","data":{"text":" "}}
        ]),
        false,
    )?;
    let step = Plugin::fetch(locator.clone(), vec![], LIMIT).map_err(|e| e.failure.message)?;
    assert_eq!(
        request_body(step)?,
        ("/get_msg".into(), json!({"message_id":-42}))
    );
    let stored = stored_message(
        10002,
        json!([
            {"type":"at","data":{"qq":"10001"}},
            {"type":"text","data":{"text":" look"}},
            {"type":"face","data":{"id":"14"}},
            {"type":"image","data":{"url":"https://multimedia.nt.qq.com.cn/download?fileid=test"}}
        ]),
    );
    let FetchStep::Messages(messages) =
        Plugin::fetch(locator, vec![api(stored)], LIMIT).map_err(|e| e.failure.message)?
    else {
        return Err("quoted messages".into());
    };
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].sender.as_deref(), Some("10002"));
    assert_eq!(messages[0].sender_name.as_deref(), Some("Nickname"));
    // Only the incoming message leaves out its leading mention of the bot.
    assert_eq!(messages[0].text, "@10001 look[face: 微笑]");
    assert_eq!(
        messages[0].sent_at.as_ref().map(|time| time.seconds),
        Some(1793291000)
    );
    assert_eq!(messages[0].images.len(), 1);
    Ok(())
}

#[test]
fn reply_to_forward_unwraps_and_nested_nodes_follow_the_parent() -> Result<(), String> {
    let locator = quote_locator(json!([{"type":"reply","data":{"id":"-42"}}]), true)?;
    let first = api(stored_message(10002, forward("outer")));
    let step = Plugin::fetch(locator.clone(), vec![first.clone()], LIMIT)
        .map_err(|e| e.failure.message)?;
    assert_eq!(request_body(step)?.0, "/get_forward_msg");
    let outer = api(json!({"messages":[
        forward_node(10003, "Group card", forward("inner")),
        forward_node(10004, "", text("last"))
    ]}));
    let inner = api(json!({"messages":[forward_node(10005, "Author", json!([
        {"type":"reply","data":{"id":"0","message":[],"qq":"10002","time":1793291000}},
        {"type":"text","data":{"text":"/cancel"}}
    ]))]}));
    let FetchStep::Messages(messages) =
        Plugin::fetch(locator, vec![first, outer, inner], LIMIT).map_err(|e| e.failure.message)?
    else {
        return Err("messages".into());
    };
    assert_eq!(
        messages
            .iter()
            .map(|m| (m.depth, m.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, ""), (1, "/cancel"), (0, "last")]
    );
    assert_eq!(messages[0].sender_name.as_deref(), Some("Group card"));
    assert_eq!(messages[1].sender.as_deref(), Some("10005"));
    assert!(messages[1].unviewable.is_empty());
    Ok(())
}

#[test]
fn nested_forwards_are_requested_layer_by_layer_and_listed_in_document_order() -> Result<(), String>
{
    let locator = quote_locator(forward("outer"), true)?;
    let layers = [
        (
            "outer",
            json!([
                forward_node(10003, "", forward("first")),
                forward_node(10004, "", forward("second"))
            ]),
        ),
        ("first", json!([forward_node(10005, "", forward("deep"))])),
        ("second", json!([forward_node(10006, "", text("second"))])),
        ("deep", json!([forward_node(10007, "", text("deep"))])),
    ];
    let mut responses = vec![];
    for (id, messages) in layers {
        let step = Plugin::fetch(locator.clone(), responses.clone(), LIMIT)
            .map_err(|e| e.failure.message)?;
        assert_eq!(request_body(step)?.1, json!({"id": id}));
        responses.push(api(json!({"messages": messages})));
    }
    let FetchStep::Messages(messages) =
        Plugin::fetch(locator, responses, LIMIT).map_err(|e| e.failure.message)?
    else {
        return Err("messages".into());
    };
    assert_eq!(
        messages
            .iter()
            .map(|m| (m.depth, m.sender.as_deref().unwrap_or_default()))
            .collect::<Vec<_>>(),
        [
            (0, "10003"),
            (1, "10005"),
            (2, "10007"),
            (0, "10004"),
            (1, "10006")
        ]
    );
    Ok(())
}

#[test]
fn forwarded_input_keeps_valid_siblings_and_stops_at_the_request_limit() -> Result<(), String> {
    let locator = quote_locator(forward("outer"), true)?;
    let responses = (1..=LIMIT).map(|level| api(json!({"messages":[
        forward_node(10003, "Group card", forward(&format!("level-{level}"))),
        forward_node(10004, "", json!([{"type":"text","data":{}},{"type":"text","data":{"text":"valid"}}])),
        {"message":"malformed node"}
    ]}))).collect();
    let FetchStep::Messages(messages) =
        Plugin::fetch(locator, responses, LIMIT).map_err(|e| e.failure.message)?
    else {
        return Err("partial expansion".into());
    };
    assert_eq!(messages.iter().map(|message| message.depth).max(), Some(7));
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.text == "valid")
            .count(),
        8
    );
    assert!(messages.iter().any(|message| message.unviewable.iter().any(|part|
        matches!(&part.reason, enco_sdk::types::UnviewableReason::Unavailable(reason) if reason.contains("8-request limit")))));
    assert!(messages.iter().any(|message| {
        message
            .unviewable
            .iter()
            .any(|part| part.what == "quoted message")
    }));
    Ok(())
}

#[test]
fn missing_reply_is_unavailable() -> Result<(), String> {
    let locator = quote_locator(json!([{"type":"reply","data":{"id":"-42"}}]), true)?;
    let step = Plugin::fetch(
        locator,
        vec![response(
            r#"{"status":"failed","retcode":1400,"data":null,"wording":"message not found"}"#,
        )],
        LIMIT,
    )
    .map_err(|e| e.failure.message)?;
    assert!(matches!(step, FetchStep::Unavailable(reason) if reason.contains("message not found")));
    Ok(())
}

#[test]
fn an_empty_merged_forward_is_unavailable() -> Result<(), String> {
    let locator = quote_locator(forward("empty"), true)?;
    let step = Plugin::fetch(locator, vec![api(json!({"messages": []}))], LIMIT)
        .map_err(|e| e.failure.message)?;
    assert!(matches!(step, FetchStep::Unavailable(reason) if reason.contains("no messages")));
    Ok(())
}

#[test]
fn an_image_url_cannot_become_a_quote_request() -> Result<(), String> {
    let reading = received(frame(
        json!([{"type":"image","data":{"url":"onebot:reply:-42"}}]),
        true,
    ))?;
    let Action::Message(content) = &reading.messages[0].action else {
        return Err("message".into());
    };
    let step = Plugin::fetch(content.images[0].locator.clone(), vec![], LIMIT)
        .map_err(|e| e.failure.message)?;
    assert!(matches!(step, FetchStep::Download(url) if url == "onebot:reply:-42"));
    Ok(())
}

#[test]
fn quote_reads_propagate_retryable_and_fatal_faults_instead_of_becoming_unavailable()
-> Result<(), String> {
    let locator = quote_locator(json!([{"type":"reply","data":{"id":"-42"}}]), true)?;
    for code in [503, 401] {
        assert!(
            Plugin::fetch(locator.clone(), vec![status(code)], LIMIT).is_err(),
            "HTTP {code}"
        );
    }
    Ok(())
}
