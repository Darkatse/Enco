use super::*;

#[test]
fn messages_keep_native_text_commands_addressing_threads_and_attachments() -> Result<(), String> {
    let mut plain = message();
    plain["content"] = json!("/cancel");
    let plain = created(1, plain)?;
    let inbound = &plain.messages[0];
    assert_eq!(inbound.key, "MESSAGE_CREATE:10");
    assert_eq!(inbound.conversation, "thread");
    assert_eq!(inbound.realm.as_deref(), Some("guild"));
    assert_eq!(
        inbound.conversation_name,
        Some(enco_sdk::channel::ConversationName::Fetch(
            "channel:thread".into()
        ))
    );
    assert!(!inbound.addressed);
    assert!(matches!(&inbound.action, Action::Message(content) if content.text == "/cancel"));
    // This bot writes in Components V2: its text is in text displays, and `content` is empty.
    let mut answer = message();
    answer["id"] = json!("8");
    answer["author"] = json!({"id": "42", "username": "Helper", "bot": true});
    answer["content"] = json!("");
    answer["flags"] = json!(32768);
    answer["components"] = json!([{"type": 17, "id": 1, "accent_color": 5793266, "components": [
        {"type": 10, "id": 2, "content": "First part"},
        {"type": 14, "id": 3, "divider": true, "spacing": 1},
        {"type": 10, "id": 4, "content": "Second part"},
    ]}]);
    let mut reply = message();
    reply["type"] = json!(19);
    reply["message_reference"] =
        json!({"type": 0, "message_id": "8", "channel_id": "thread", "guild_id": "guild"});
    reply["referenced_message"] = answer;
    reply["attachments"] = json!([
        {
            "filename": "image.webp",
            "url": "https://cdn.discordapp.com/attachments/a.webp",
            "content_type": "image/webp",
        },
        {
            "filename": "voice.ogg",
            "url": "https://cdn.discordapp.com/attachments/a.ogg",
            "content_type": "audio/ogg",
        },
        {"filename":"report.pdf", "url":"https://cdn.discordapp.com/attachments/report.pdf", "content_type":"application/pdf"},
        {"filename":"diagram.png", "url":"https://cdn.discordapp.com/attachments/diagram.png", "content_type":"application/octet-stream"},
    ]);
    let reply = created(2, reply)?;
    let inbound = &reply.messages[0];
    assert!(inbound.addressed);
    assert_eq!(inbound.reply_to.as_deref(), Some("8"));
    assert_eq!(inbound.sender_name.as_deref(), Some("Ada"));
    let Action::Message(content) = &inbound.action else {
        return Err("expected content".into());
    };
    assert_eq!(
        content.attachments[0].locator,
        "https://cdn.discordapp.com/attachments/a.webp"
    );
    assert_eq!(content.attachments[1].name, "voice.ogg");
    assert_eq!(content.attachments[2].name, "report.pdf");
    assert_eq!(content.attachments[3].name, "diagram.png");
    assert_eq!(
        Plugin::fetch(
            content.attachments[2].locator.clone(),
            vec![HttpResponse {
                status: 200,
                headers: vec![],
                body: b"PDF bytes".to_vec()
            }],
            1
        ),
        Ok(crate::channel::FetchStep::File(b"PDF bytes".to_vec()))
    );
    assert!(content.unviewable.is_empty());
    let Some(Quote {
        messages: Quoted::Messages(quoted),
        ..
    }) = &content.quote
    else {
        return Err("expected the replied message".into());
    };
    assert_eq!(quoted[0].text, "First part\nSecond part");
    let mut direct = message();
    direct
        .as_object_mut()
        .ok_or("message object")?
        .remove("guild_id");
    let direct = created(3, direct)?;
    assert!(direct.messages[0].direct);
    assert!(direct.messages[0].realm.is_none() && direct.messages[0].conversation_name.is_none());
    let mut bot = message();
    bot["author"]["bot"] = json!(true);
    assert!(created(4, bot)?.messages.is_empty());
    Ok(())
}

#[test]
fn only_contentless_messages_are_ignored_and_forwards_are_not_replies() -> Result<(), String> {
    let mut sticker = message();
    sticker["content"] = json!("");
    assert!(created(1, sticker.clone())?.messages.is_empty());
    sticker["sticker_items"] = json!([{"id": "1", "name": "wave", "format_type": 1}]);
    // A link preview repeats a URL from the text; only the rich embed is something the sender wrote.
    sticker["embeds"] = json!([
        {"type": "link", "url": "https://example.com/trip"},
        {"type": "rich", "title": "Trip plan", "description": "Saturday at eight"},
    ]);
    let reading = created(1, sticker)?;
    let Action::Message(content) = &reading.messages[0].action else {
        return Err("expected content".into());
    };
    assert_eq!(
        content
            .unviewable
            .iter()
            .map(|part| part.what.as_str())
            .collect::<Vec<_>>(),
        ["sticker", "card"]
    );
    let mut poll = message();
    poll["content"] = json!("");
    poll["poll"] = json!({"question":{"text":"When?"}, "answers":[
        {"answer_id":1,"poll_media":{"text":"Today"}},
        {"answer_id":2,"poll_media":{"text":"Tomorrow"}},
    ], "expiry":"2026-11-04T12:00:00+00:00", "allow_multiselect":false, "layout_type":1});
    let reading = created(2, poll)?;
    let Action::Message(content) = &reading.messages[0].action else {
        return Err("expected poll".into());
    };
    assert!(content.attachments.is_empty());
    assert_eq!(content.unviewable[0].what, "poll");
    let mut forwarded = message();
    forwarded["content"] = json!("");
    forwarded["message_reference"] =
        json!({"type": 1, "message_id": "8", "channel_id": "elsewhere", "guild_id": "guild"});
    forwarded["message_snapshots"] = json!([{"message": {
        "type": 0, "content": "hello", "timestamp": "2026-10-28T12:00:00.000000+00:00",
        "mentions": [], "attachments": [], "embeds": [], "components": [], "flags": 0,
    }}]);
    let inbound = &created(2, forwarded)?.messages[0];
    assert!(matches!(&inbound.action, Action::Message(content) if content.quote.is_some()));
    assert_eq!(inbound.reply_to, None);
    Ok(())
}

#[test]
fn native_commands_and_buttons_defer_ephemerally_then_answer() -> Result<(), String> {
    let option = |name, value| json!([{"name": name, "value": value}]);
    let named = |command| json!({"name": command, "options": option("name", "main/research")});
    for (kind, data, action) in [
        (2, json!({"name": "cancel"}), Action::Cancel),
        (2, json!({"name": "status"}), Action::Status),
        (2, json!({"name": "compact"}), Action::Compact),
        (2, json!({"name": "clear"}), Action::Clear),
        (3, json!({"custom_id": "cancel"}), Action::Cancel),
        (
            2,
            json!({"name": "Backtrack here", "target_id": "previous"}),
            Action::Backtrack,
        ),
        (
            2,
            json!({"name": "session", "options": option("name", "work")}),
            Action::SelectSession("work".into()),
        ),
        (
            2,
            json!({"name": "everyone", "options": option("open", "off")}),
            Action::Everyone(Some(false)),
        ),
        (2, named("halt"), Action::Halt(Some("main/research".into()))),
        (
            2,
            named("archive"),
            Action::Archive(Some("main/research".into())),
        ),
    ] {
        let interaction = json!({
            "id": "interaction",
            "application_id": "app",
            "token": "continuation",
            "type": kind,
            "channel_id": "conversation",
            "member": {"user": {"id": "7", "username": "ada"}},
            "guild_id": "guild",
            "data": data,
        });
        let reading = frame(
            None,
            json!({"op": 0, "s": 2, "t": "INTERACTION_CREATE", "d": interaction}),
        )?;
        let inbound = &reading.messages[0];
        assert_eq!(inbound.key, "INTERACTION_CREATE:interaction");
        assert_eq!(inbound.realm.as_deref(), Some("guild"));
        assert_eq!(
            inbound.conversation_name,
            Some(enco_sdk::channel::ConversationName::Fetch(
                "channel:conversation".into()
            ))
        );
        assert_eq!(inbound.reply_to.is_some(), action == Action::Backtrack);
        assert_eq!(inbound.action, action);
        assert_eq!(
            body(&reading.acknowledgments[0])?,
            json!({"type":5,"data":{"flags":64}})
        );
        assert_eq!(
            reading.acknowledgments[0].url,
            "/interactions/interaction/continuation/callback"
        );
        let answer = Plugin::respond(
            inbound.respond.clone().ok_or("response handle")?,
            Some("Only the owner can use this.".into()),
        )
        .map_err(|error| error.failure.message)?
        .ok_or("missing answer")?;
        assert_eq!(answer.method, "PATCH");
        assert_eq!(answer.url, "/webhooks/app/continuation/messages/@original");
        assert_eq!(body(&answer)?["content"], "Only the owner can use this.");
    }
    Ok(())
}

#[test]
fn command_menus_have_native_contexts_and_name_their_options() -> Result<(), String> {
    let cmd = |kind, direct, group| Command {
        kind,
        description: "Manage Session".into(),
        direct,
        group,
    };
    let menu = Plugin::commands(
        identity()?,
        vec![
            cmd(CommandKind::Session, true, false),
            cmd(CommandKind::Backtrack, true, true),
            cmd(CommandKind::Halt, true, true),
            cmd(CommandKind::Archive, true, false),
            cmd(CommandKind::Status, true, true),
            cmd(CommandKind::Compact, true, true),
            cmd(CommandKind::Clear, true, true),
        ],
    );
    assert_eq!(menu[0].url, "/applications/app/commands");
    let commands = body(&menu[0])?;
    assert_eq!(commands[0]["contexts"], json!([0, 1]));
    assert_eq!(commands[3]["contexts"], json!([0, 1]));
    assert_eq!(commands[1]["type"], 3);
    assert!(commands[1].get("description").is_none());
    for (index, name) in [(0, "session"), (2, "halt"), (3, "archive")] {
        assert_eq!(commands[index]["name"], name);
        assert_eq!(commands[index]["options"][0]["name"], "name");
        assert_eq!(commands[index]["options"][0]["required"], false);
    }
    for (index, name) in [(4, "status"), (5, "compact"), (6, "clear")] {
        assert_eq!(commands[index]["name"], name);
        assert_eq!(commands[index]["type"], 1);
        assert_eq!(commands[index]["options"], json!([]));
    }
    Ok(())
}
