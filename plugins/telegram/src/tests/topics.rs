use super::*;
use enco_sdk::channel::ConversationName;

fn updates(state: Option<String>, items: Value) -> Result<super::super::channel::Reading, String> {
    let body = json!({"ok": true, "result": items}).to_string();
    Plugin::received(
        identity()?,
        state,
        Arrival::Response(response(200, body.as_bytes())),
    )
    .map_err(|fault| fault.failure.message)
}

fn payload(request: &super::super::channel::Request) -> Result<Value, String> {
    serde_json::from_slice(request.body.as_deref().ok_or("missing body")?)
        .map_err(|e| e.to_string())
}

/// The owner, as Telegram reports the sender of the owner's messages in a private chat.
fn owner() -> Value {
    json!({"id": 42, "is_bot": false, "first_name": "Owner"})
}

#[test]
fn topic_titles_are_handed_over_once_and_service_messages_are_neither_inputs_nor_quotes()
-> Result<(), String> {
    let created = json!({"message_id": 7, "date": 1760000000, "message_thread_id": 7,
        "chat": {"id": 42, "type": "private"}, "from": owner(),
        "forum_topic_created": {"name": "Research", "icon_color": 13338331, "is_name_implicit": true}});
    let first = updates(
        None,
        json!([
            {"update_id": 1, "message": created},
            {"update_id": 2, "message": {"message_id": 8, "date": 1760000001,
                "chat": {"id": 42, "type": "private"}, "message_thread_id": 7, "from": owner(),
                "text": "look for papers", "reply_to_message": created}}
        ]),
    )?;
    assert_eq!(first.messages.len(), 1);
    let message = &first.messages[0];
    assert_eq!(message.conversation, "42:7");
    assert_eq!(message.realm.as_deref(), Some("42"));
    assert!(message.direct && message.addressed);
    assert_eq!(
        message.conversation_name,
        Some(ConversationName::Name("Research".into()))
    );
    assert!(message.reply_to.is_none());
    let Action::Message(content) = &message.action else {
        return Err("message".into());
    };
    assert!(content.quote.is_none());
    let renamed = updates(
        first.state,
        json!([
            {"update_id": 3, "message": {"message_id": 9, "date": 1760000002,
                "chat": {"id": 42, "type": "private"}, "message_thread_id": 7, "from": owner(),
                "forum_topic_edited": {"name": "Papers"}}},
            {"update_id": 4, "message": {"message_id": 10, "date": 1760000003,
                "chat": {"id": 42, "type": "private"}, "message_thread_id": 7, "from": owner(),
                "text": "continue"}}
        ]),
    )?;
    assert_eq!(renamed.messages.len(), 1);
    assert_eq!(
        renamed.messages[0].conversation_name,
        Some(ConversationName::Name("Papers".into()))
    );
    let next = updates(
        renamed.state,
        json!([
            {"update_id": 5, "message": {"message_id": 11, "date": 1760000004,
                "chat": {"id": 42, "type": "private"}, "message_thread_id": 7, "from": owner(),
                "text": "more"}}
        ]),
    )?;
    assert!(next.messages[0].conversation_name.is_none());
    Ok(())
}

#[test]
fn a_private_topic_is_in_the_chats_realm_even_without_a_known_title() -> Result<(), String> {
    let reading = updates(
        None,
        json!([
            {"update_id": 1, "message": {"message_id": 8, "date": 1760000000,
                "chat": {"id": 42, "type": "private"}, "message_thread_id": 7, "from": owner(),
                "text": "in a topic"}},
            {"update_id": 2, "message": {"message_id": 9, "date": 1760000001,
                "chat": {"id": 42, "type": "private"}, "from": owner(), "text": "no topic"}}
        ]),
    )?;
    let [topic, chat] = &reading.messages[..] else {
        return Err("two messages".into());
    };
    assert_eq!(topic.conversation, "42:7");
    assert_eq!(topic.realm.as_deref(), Some("42"));
    assert!(topic.conversation_name.is_none());
    assert_eq!(chat.conversation, "42");
    assert!(chat.realm.is_none() && chat.conversation_name.is_none());
    Ok(())
}

#[test]
fn group_topics_and_callback_cancellation_use_the_same_conversation_identity() -> Result<(), String>
{
    let ada = json!({"id": 7, "is_bot": false, "first_name": "Ada"});
    let reading = updates(
        None,
        json!([
            {"update_id": 1, "message": {"message_id": 7, "date": 1760000000,
                "chat": {"id": -100, "type": "supergroup", "title": "Group"}, "message_thread_id": 7,
                "is_topic_message": true, "from": ada, "text": "question"}},
            {"update_id": 2, "message": {"message_id": 8, "date": 1760000001,
                "chat": {"id": -100, "type": "supergroup", "title": "Group"}, "message_thread_id": 7,
                "from": ada, "text": "ordinary reply"}},
            {"update_id": 3, "callback_query": {"id": "cancel-id", "chat_instance": "instance",
                "from": {"id": 42, "is_bot": false, "first_name": "Owner"}, "data": "cancel",
                "message": {"message_id": 9, "date": 1760000002, "message_thread_id": 7,
                    "is_topic_message": true, "chat": {"id": -100, "type": "supergroup", "title": "Group"}}}},
            {"update_id": 4, "message": {"message_id": 10, "date": 1760000003,
                "chat": {"id": -100, "type": "supergroup", "title": "Group"},
                "from": {"id": 77, "is_bot": true, "first_name": "Other bot"}, "text": "ignore me"}}
        ]),
    )?;
    assert_eq!(
        reading
            .messages
            .iter()
            .map(|m| (m.conversation.as_str(), m.realm.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("-100:7", Some("-100")),
            ("-100", None),
            ("-100:7", Some("-100"))
        ]
    );
    assert!(matches!(reading.messages[2].action, Action::Cancel));
    Ok(())
}

#[test]
fn stopping_a_private_draft_cancels_the_topic_and_is_requested_in_polling() -> Result<(), String> {
    let ReadStep::Request(poll) = Plugin::read(identity()?, None).map_err(|f| f.failure.message)?
    else {
        return Err("poll".into());
    };
    assert_eq!(
        payload(&poll)?["allowed_updates"],
        json!(["message", "callback_query", "stopped_message_generation"])
    );
    let stopped = |thread: Option<i64>| {
        let mut stopped = json!({"chat": {"id": 42, "type": "private"}, "draft_id": 1});
        if let Some(thread) = thread {
            stopped["message_thread_id"] = json!(thread);
        }
        stopped
    };
    let reading = updates(
        None,
        json!([
            {"update_id": 30, "stopped_message_generation": stopped(Some(7))},
            {"update_id": 31, "stopped_message_generation": stopped(Some(7))},
            {"update_id": 32, "stopped_message_generation": stopped(None)}
        ]),
    )?;
    let [first, second, chat] = &reading.messages[..] else {
        return Err("three stops".into());
    };
    assert_eq!(
        [first, second, chat].map(|m| (m.conversation.as_str(), m.realm.as_deref())),
        [("42:7", Some("42")), ("42:7", Some("42")), ("42", None)]
    );
    // The draft is always the same; each stop is a message of its own.
    assert_ne!(first.message, second.message);
    assert!(matches!(first.action, Action::Cancel));
    assert_eq!(first.sender.as_deref(), Some("42"));
    assert!(first.direct && first.addressed && first.respond.is_none());
    Ok(())
}

#[test]
fn child_topics_open_in_private_chats_and_are_deleted_on_close() -> Result<(), String> {
    let names = vec!["main".into(), "research".into()];
    let opened = Plugin::open_conversation("42:7".into(), names.clone()).ok_or("open")?;
    assert_eq!(opened.url, "/bot/createForumTopic");
    let body = payload(&opened)?;
    assert_eq!(body["chat_id"], "42");
    assert_eq!(body["name"], "main › research");
    // Child topics carry an icon colour of their own, apart from the ones people open.
    assert!(body["icon_color"].is_i64());
    assert!(Plugin::open_conversation("-100:7".into(), names).is_none());
    let id = Plugin::opened("42:7".into(), response(200, r#"{"ok":true,"result":{"message_thread_id":9,"name":"main › research","icon_color":13338331}}"#.as_bytes()))
        .map_err(|f| f.failure.message)?;
    assert_eq!(id, "42:9");
    let close = Plugin::close_conversation(id).ok_or("close")?;
    assert_eq!(close.url, "/bot/deleteForumTopic");
    assert_eq!(
        payload(&close)?,
        json!({"chat_id": "42", "message_thread_id": 9})
    );
    Ok(())
}
