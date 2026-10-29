use super::{
    Plugin,
    channel::{Arrival, Closure, FetchStep, FileData, Guest, Identity, Outgoing, Response},
    fixtures,
};
use enco_sdk::channel::{Action, ActivityView};
use serde_json::{Value, json};

fn response(body: &str) -> Response {
    Response {
        status: 200,
        headers: vec![],
        body: body.as_bytes().into(),
    }
}

fn status(status: u16) -> Response {
    Response {
        status,
        ..response("")
    }
}

fn identity() -> Result<Identity, String> {
    Plugin::identified(response(fixtures::IDENTITY)).map_err(|error| error.failure.message)
}

fn frame(segments: Value, direct: bool) -> Value {
    json!({
        "post_type": "message", "message_type": if direct { "private" } else { "group" },
        "message_id": -7, "user_id": 10002_u64, "group_id": 20001,
        "sender": {"nickname": "Owner", "card": "Group name"}, "message": segments,
    })
}

fn received(value: Value) -> Result<super::channel::Reading, String> {
    Plugin::received(identity()?, None, Arrival::Frame(value.to_string()))
        .map_err(|error| error.failure.message)
}

#[test]
fn probe_samples_keep_signed_ids_and_native_group_addressing() -> Result<(), String> {
    assert_eq!(identity()?.account, "10001");
    assert_eq!(
        Plugin::answered(response(fixtures::PRIVATE_SENT)).map_err(|e| e.failure.message)?,
        Some("-987654321".into())
    );
    assert_eq!(
        Plugin::answered(response(fixtures::GROUP_SENT)).map_err(|e| e.failure.message)?,
        Some("-123456789".into())
    );
    let mut value: Value =
        serde_json::from_str(fixtures::GROUP_REPLY).map_err(|e| e.to_string())?;
    let reading = received(value.clone())?;
    let message = &reading.messages[0];
    assert_eq!(message.conversation, "group:20001");
    assert_eq!(message.key, "group:20001/-123456788");
    assert_eq!(message.reply_to.as_deref(), Some("-123456789"));
    assert!(message.addressed && !message.direct);
    assert!(matches!(&message.action, Action::Message(content) if content.text == "这次再试试"));
    value["message"].as_array_mut().ok_or("segments")?.remove(1);
    assert!(!received(value)?.messages[0].addressed);
    Ok(())
}

#[test]
fn commands_follow_a_leading_mention_and_attachments_keep_them_as_messages() -> Result<(), String> {
    for (text, expected) in [
        (" /session work", Action::SelectSession("work".into())),
        (" /cancel", Action::Cancel),
    ] {
        let reading = received(frame(
            json!([
                {"type":"at","data":{"qq":"010001"}},
                {"type":"text","data":{"text": text}},
            ]),
            false,
        ))?;
        assert_eq!(reading.messages[0].action, expected);
        assert!(reading.messages[0].addressed);
    }
    let reading = received(frame(
        json!([{"type":"text","data":{"text":"／cancel"}}]),
        true,
    ))?;
    assert!(
        matches!(&reading.messages[0].action, Action::Message(content) if content.text == "／cancel")
    );
    let reading = received(frame(
        json!([
            {"type":"text","data":{"text":"/cancel"}},
            {"type":"file","data":{"name":"report.pdf"}},
        ]),
        true,
    ))?;
    assert!(
        matches!(&reading.messages[0].action, Action::Message(content) if content.unviewable.len() == 1)
    );
    let reading = received(frame(
        json!([
            {"type":"reply","data":{"id":"-123456789"}},
            {"type":"text","data":{"text":"/backtrack"}},
        ]),
        false,
    ))?;
    assert!(matches!(reading.messages[0].action, Action::Backtrack));
    assert!(reading.messages[0].addressed);
    Ok(())
}

#[test]
fn ignored_events_own_messages_and_closure_yield_nothing() -> Result<(), String> {
    for value in [
        json!({"post_type":"meta_event","meta_event_type":"heartbeat"}),
        json!({"post_type":"notice"}),
        json!({"post_type":"message_sent"}),
    ] {
        assert!(received(value)?.messages.is_empty());
    }
    let mut own = frame(json!([{"type":"text","data":{"text":"echo"}}]), true);
    own["user_id"] = json!(10001_u64);
    assert!(received(own)?.messages.is_empty());
    let mention_only = frame(
        json!([
            {"type":"at","data":{"qq":"10001"}},
            {"type":"text","data":{"text":" "}},
        ]),
        false,
    );
    assert!(received(mention_only)?.messages.is_empty());
    let reading = Plugin::received(
        identity()?,
        None,
        Arrival::Closed(Closure {
            code: Some(1006),
            reason: "connection lost".into(),
        }),
    )
    .map_err(|e| e.failure.message)?;
    assert!(reading.messages.is_empty());
    Ok(())
}

#[test]
fn images_and_unsupported_segments_preserve_attachment_evidence() -> Result<(), String> {
    let reading = received(frame(
        json!([
            {"type":"text","data":{"text":"look"}},
            {"type":"at","data":{"qq":"all"}},
            {"type":"face","data":{"id":"14"}},
            {"type":"image","data":{"url":"https://multimedia.nt.qq.com.cn/download?fileid=x"}},
            {"type":"record","data":{"file":"voice"}},
            {"type":"video","data":{"file":"video"}},
            {"type":"json","data":{"data":"{}"}},
        ]),
        true,
    ))?;
    let Action::Message(content) = &reading.messages[0].action else {
        return Err("message".into());
    };
    assert_eq!(content.text, "look@all[face:14]");
    assert_eq!(content.images.len(), 1);
    assert_eq!(content.unviewable.len(), 3);
    let locator = content.images[0].locator.clone();
    // The address came from the message, so the host reaches it as content, without credentials.
    assert!(
        matches!(Plugin::fetch(locator.clone(), vec![]).map_err(|e| e.failure.message)?, FetchStep::Download(url) if url == locator)
    );
    assert!(
        matches!(Plugin::fetch(locator, vec![response("image bytes")]).map_err(|e| e.failure.message)?, FetchStep::Image(bytes) if bytes == b"image bytes")
    );
    Ok(())
}

#[test]
fn image_download_status_never_judges_the_connection() -> Result<(), String> {
    let locator = "https://multimedia.nt.qq.com.cn/download?fileid=x";
    for code in [400, 401, 403, 404, 410] {
        let step =
            Plugin::fetch(locator.into(), vec![status(code)]).map_err(|e| e.failure.message)?;
        assert!(
            matches!(&step, FetchStep::Unavailable(reason) if reason.contains(&code.to_string())),
            "HTTP {code}"
        );
    }
    for code in [429, 500, 503] {
        let fault = Plugin::fetch(locator.into(), vec![status(code)])
            .err()
            .ok_or("retryable download")?;
        assert!(
            fault.failure.retryable && !fault.fatal && !fault.unknown,
            "HTTP {code}"
        );
    }
    Ok(())
}

#[test]
fn api_status_separates_token_rejection_retries_and_unknown_delivery() -> Result<(), String> {
    for (code, fatal, retryable, unknown) in [
        (401, true, false, false),
        (403, true, false, false),
        (400, false, false, false),
        (404, false, false, false),
        (429, false, true, false),
        (500, false, true, true),
        (503, false, true, true),
    ] {
        let fault = Plugin::answered(status(code)).err().ok_or("HTTP failure")?;
        assert_eq!(
            (fault.fatal, fault.failure.retryable, fault.unknown),
            (fatal, retryable, unknown),
            "HTTP {code}"
        );
    }
    Ok(())
}

#[test]
fn required_external_fields_and_array_configuration_fail_with_context() -> Result<(), String> {
    let string_message = frame(json!("[CQ:at,qq=10001] hello"), true);
    assert!(
        received(string_message)
            .err()
            .ok_or("error")?
            .contains("messageFormat")
    );
    for (segments, context) in [
        (json!([{"type":"text","data":{}}]), "text segment data.text"),
        (
            json!([{"type":"reply","data":{"id":"no"}}]),
            "reply segment data.id",
        ),
        (
            json!([{"type":"at","data":{"qq":"no"}}]),
            "at segment data.qq",
        ),
    ] {
        assert!(
            received(frame(segments, true))
                .err()
                .ok_or("error")?
                .contains(context)
        );
    }
    let mut malformed = frame(json!([]), false);
    malformed.as_object_mut().ok_or("event")?.remove("group_id");
    assert!(
        received(malformed)
            .err()
            .ok_or("error")?
            .contains("group_id")
    );
    Ok(())
}

#[test]
fn split_preserves_markdown_and_separates_only_non_image_files() -> Result<(), String> {
    let text = format!(
        "{}\n```rust\n{}\n```\n",
        "中".repeat(3990),
        "x".repeat(6000)
    );
    for (data, is_image) in [
        (b"\x89PNG\r\n\x1a\nimage".to_vec(), true),
        (b"report bytes".to_vec(), false),
    ] {
        let file = FileData {
            name: "report.bin".into(),
            data: data.clone(),
        };
        let parts = Plugin::split(Outgoing {
            text: text.clone(),
            file: Some(file.clone()),
        });
        assert_eq!(
            parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(parts.iter().all(|part| part.text.chars().count() <= 4000));
        assert!(parts[0].text.ends_with('\n'));
        let last = parts.last().ok_or("parts")?;
        assert_eq!(last.text.is_empty(), !is_image);
        let request =
            Plugin::send("group:20001".into(), last.clone()).map_err(|e| e.failure.message)?;
        let body: Value =
            serde_json::from_slice(&request.body.ok_or("body")?).map_err(|e| e.to_string())?;
        assert_eq!(body["group_id"], 20001);
        if is_image {
            assert_eq!(request.url, "/send_group_msg");
            assert_eq!(
                body["message"][1]["data"]["file"],
                "base64://iVBORw0KGgppbWFnZQ=="
            );
        } else {
            assert_eq!(request.url, "/upload_group_file");
            assert_eq!(body["file"], "base64://cmVwb3J0IGJ5dGVz");
            assert_eq!(body["name"], "report.bin");
            let with_text = Outgoing {
                text: "notes".into(),
                file: Some(file),
            };
            assert!(Plugin::send("group:20001".into(), with_text).is_err());
        }
    }
    let request = Plugin::send(
        "private:10002".into(),
        Outgoing {
            text: "hello".into(),
            file: None,
        },
    )
    .map_err(|e| e.failure.message)?;
    assert_eq!(request.url, "/send_private_msg");
    Ok(())
}

#[test]
fn uploaded_files_and_typing_settle_without_a_message_id() -> Result<(), String> {
    for body in [
        r#"{"status":"ok","retcode":0,"data":{"file_id":"uploaded-file-uuid"}}"#,
        r#"{"status":"ok","retcode":0,"data":{}}"#,
        r#"{"status":"ok","retcode":0,"data":null}"#,
        r#"{"status":"ok","retcode":0,"data":{"message_id":0}}"#,
    ] {
        assert!(
            Plugin::answered(response(body))
                .map_err(|e| e.failure.message)?
                .is_none()
        );
    }
    let failure = Plugin::answered(response(
        r#"{"status":"failed","retcode":1400,"data":null,"wording":"user not found"}"#,
    ))
    .err()
    .ok_or("failure")?;
    assert!(!failure.unknown && !failure.failure.retryable);
    assert!(failure.failure.message.contains("1400"));
    let ambiguous = Plugin::answered(response("bad JSON"))
        .err()
        .ok_or("failure")?;
    assert!(ambiguous.unknown);
    for data in [
        json!({"message_id": "-123456789"}),
        json!({"message_id": {}}),
        json!({"message_id": 2_147_483_648_u64}),
    ] {
        let body = json!({"status": "ok", "retcode": 0, "data": data}).to_string();
        let malformed = Plugin::answered(response(&body))
            .err()
            .ok_or("invalid message ID")?;
        assert!(malformed.unknown && !malformed.failure.retryable);
        assert!(malformed.failure.message.contains("data.message_id"));
    }
    let request = Plugin::presence("private:10002".into()).ok_or("typing")?;
    assert_eq!(request.url, "/set_input_status");
    let body: Value =
        serde_json::from_slice(&request.body.ok_or("body")?).map_err(|e| e.to_string())?;
    assert_eq!(body, json!({"user_id": 10002, "event_type": 1}));
    assert!(Plugin::presence("group:20001".into()).is_none());
    let activity = ActivityView {
        elapsed_seconds: 0,
        tools: vec![],
    };
    assert!(Plugin::activity("private:10002".into(), None, Some(activity)).is_none());
    Ok(())
}
