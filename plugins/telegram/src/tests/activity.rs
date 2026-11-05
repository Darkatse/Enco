use super::*;
use enco_sdk::channel::{ActivityEpilogue, RunEnd, ToolActivity, ToolState};

fn view() -> ActivityView {
    ActivityView {
        elapsed_seconds: 3,
        tools: vec![ToolActivity {
            name: "read_file".into(),
            arguments: "[a]*.rs".into(),
            state: ToolState::Running,
        }],
    }
}

fn body(request: &super::super::channel::HttpRequest) -> Result<Value, String> {
    serde_json::from_slice(request.body.as_deref().ok_or("body")?).map_err(|e| e.to_string())
}

#[test]
fn private_waiters_use_stoppable_plain_drafts_while_background_and_groups_use_cards()
-> Result<(), String> {
    let draft = Plugin::presence("42:7".into(), view()).ok_or("draft")?;
    assert_eq!(draft.url, "/bot/sendMessageDraft");
    let payload = body(&draft)?;
    assert_eq!(payload["chat_id"], "42");
    assert_eq!(payload["message_thread_id"], 7);
    assert_eq!(payload["can_stop"], true);
    // Plain text: the arguments' Markdown characters show as written.
    let text = payload["text"].as_str().ok_or("draft text")?;
    assert!(text.contains("read_file") && text.contains("[a]*.rs"));
    assert!(payload.get("parse_mode").is_none());
    assert!(Plugin::activity("42:7".into(), true, None, ActivityUpdate::View(view())).is_none());
    for (id, replies_here) in [("42:7", false), ("-100:7", true)] {
        let card = Plugin::activity(id.into(), replies_here, None, ActivityUpdate::View(view()))
            .ok_or("card")?;
        assert_eq!(card.url, "/bot/sendMessage");
        let payload = body(&card)?;
        assert_eq!(payload["message_thread_id"], 7);
        assert_eq!(payload["disable_notification"], true);
        assert_eq!(
            payload["reply_markup"]["inline_keyboard"][0][0]["style"],
            "danger"
        );
    }
    let typing = Plugin::presence("-100:7".into(), view()).ok_or("typing")?;
    assert_eq!(typing.url, "/bot/sendChatAction");
    assert_eq!(
        body(&typing)?,
        json!({"chat_id": "-100", "message_thread_id": 7, "action": "typing"})
    );
    Ok(())
}

#[test]
fn epilogues_remove_cancel_controls_and_only_new_cards_notify_silently() -> Result<(), String> {
    let epilogue = ActivityEpilogue {
        elapsed_seconds: 420,
        rounds: 6,
        end: RunEnd::Completed,
        unsent: false,
        excerpt: "Papers found".into(),
    };
    let edit = Plugin::activity(
        "42:7".into(),
        false,
        Some("8".into()),
        ActivityUpdate::Epilogue(epilogue.clone()),
    )
    .ok_or("edit")?;
    assert_eq!(edit.url, "/bot/editMessageText");
    let payload = body(&edit)?;
    assert_eq!(payload["message_id"], "8");
    assert_eq!(payload["reply_markup"]["inline_keyboard"], json!([]));
    assert!(payload.get("disable_notification").is_none());
    let create = Plugin::activity(
        "42:7".into(),
        false,
        None,
        ActivityUpdate::Epilogue(epilogue),
    )
    .ok_or("create")?;
    assert_eq!(create.url, "/bot/sendMessage");
    let payload = body(&create)?;
    assert_eq!(payload["message_thread_id"], 7);
    assert_eq!(payload["disable_notification"], true);
    assert!(payload.get("reply_markup").is_none());
    assert!(Plugin::activity("42:7".into(), false, None, ActivityUpdate::Remove).is_none());
    Ok(())
}

#[test]
fn sends_route_both_json_and_multipart_to_the_topic() -> Result<(), String> {
    let send = Plugin::send(
        "42:7".into(),
        Outgoing {
            text: "reply".into(),
            file: None,
        },
    )
    .map_err(|f| f.failure.message)?;
    let payload = body(&send)?;
    assert_eq!(payload["chat_id"], "42");
    assert_eq!(payload["message_thread_id"], 7);
    let send = Plugin::send(
        "42:7".into(),
        Outgoing {
            text: "reply".into(),
            file: Some(FileData {
                name: "data.bin".into(),
                data: vec![1, 2],
            }),
        },
    )
    .map_err(|f| f.failure.message)?;
    let payload = String::from_utf8(send.body.ok_or("multipart")?).map_err(|e| e.to_string())?;
    assert!(payload.contains("name=\"chat_id\"\r\n\r\n42\r\n"));
    assert!(payload.contains("name=\"message_thread_id\"\r\n\r\n7\r\n"));
    Ok(())
}

#[test]
fn missing_topics_and_chats_are_distinct_from_missing_messages() -> Result<(), String> {
    for description in [
        "Bad Request: message thread not found",
        "Bad Request: chat not found",
        "Bad Request: TOPIC_DELETED",
        "Bad Request: TOPIC_ID_INVALID",
    ] {
        let error = json!({"ok": false, "error_code": 400, "description": description}).to_string();
        let fault = Plugin::answered(response(400, error.as_bytes()))
            .err()
            .ok_or("fault")?;
        assert_eq!(fault.failure.code, "channel.conversation_gone");
        assert!(!fault.unknown && !fault.fatal && !fault.failure.retryable);
    }
    let error =
        br#"{"ok":false,"error_code":400,"description":"Bad Request: message to edit not found"}"#;
    let fault = Plugin::answered(response(400, error))
        .err()
        .ok_or("missing message")?;
    assert_eq!(fault.failure.code, "channel.http.400");
    Ok(())
}
