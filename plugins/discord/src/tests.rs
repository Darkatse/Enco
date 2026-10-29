use super::{
    Plugin,
    channel::{
        Arrival, Closure, Command, FileData, Guest, Identity, Outgoing, ReadStep, Reading, Request,
        Response,
    },
};
use enco_sdk::channel::{Action, CommandKind};
use enco_sdk::channel::{ActivityView, ToolActivity, ToolState};
use serde_json::{Value, json};

fn response(status: u16, body: Value) -> Response {
    Response {
        status,
        headers: vec![],
        body: body.to_string().into_bytes(),
    }
}

fn identity() -> Result<Identity, String> {
    Plugin::identified(response(
        200,
        json!({"id":"app","bot":{"id":"42","username":"Helper"}}),
    ))
    .map_err(|error| error.failure.message)
}

fn arrival(state: Option<String>, arrival: Arrival) -> Result<Reading, String> {
    Plugin::received(identity()?, state, arrival).map_err(|error| error.failure.message)
}

fn frame(state: Option<String>, frame: Value) -> Result<Reading, String> {
    arrival(state, Arrival::Frame(frame.to_string()))
}

fn body(request: &Request) -> Result<Value, String> {
    serde_json::from_slice(request.body.as_deref().ok_or("missing body")?)
        .map_err(|error| error.to_string())
}

fn message() -> Value {
    json!({
        "id": "10",
        "channel_id": "thread",
        "guild_id": "guild",
        "type": 0,
        "author": {"id": "7", "username": "ada", "global_name": "Ada"},
        "content": "hello",
        "mentions": [],
        "attachments": [],
    })
}

fn created(sequence: u64, message: Value) -> Result<Reading, String> {
    frame(
        None,
        json!({"op": 0, "s": sequence, "t": "MESSAGE_CREATE", "d": message}),
    )
}

#[test]
fn gateway_heartbeat_detects_missing_ack_despite_continuing_dispatches() -> Result<(), String> {
    let hello = frame(None, json!({"op":10,"d":{"heartbeat_interval":40_000}}))?;
    assert!(hello.timer.is_some_and(|delay| delay < 40_000));
    let identify: Value =
        serde_json::from_str(&hello.frames[0]).map_err(|error| error.to_string())?;
    assert_eq!(identify["op"], 2);
    assert!(identify["d"]["token"].is_null());
    assert_eq!(
        identify["d"]["intents"].as_u64().ok_or("intents")? & (1 << 15),
        1 << 15
    );
    let heartbeat = arrival(hello.state, Arrival::Timer)?;
    assert_eq!(heartbeat.timer, Some(40_000));
    assert_eq!(heartbeat.frames, [r#"{"d":null,"op":1}"#]);
    let dispatch = frame(
        heartbeat.state,
        json!({"op":0,"s":1,"t":"GUILD_CREATE","d":{}}),
    )?;
    assert!(dispatch.messages.is_empty());
    let dead = arrival(dispatch.state, Arrival::Timer)?;
    assert!(dead.reconnect);
    let ack = frame(heartbeat_state()?, json!({"op":11}))?;
    let live = arrival(ack.state, Arrival::Timer)?;
    assert!(!live.reconnect);
    Ok(())
}

fn heartbeat_state() -> Result<Option<String>, String> {
    let hello = frame(None, json!({"op":10,"d":{"heartbeat_interval":1000}}))?;
    Ok(arrival(hello.state, Arrival::Timer)?.state)
}

#[test]
fn ready_disconnect_resume_and_invalid_session_follow_protocol_state() -> Result<(), String> {
    let ready = json!({
        "session_id": "session",
        "resume_gateway_url": "wss://gateway-us.discord.gg/",
    });
    let ready = frame(None, json!({"op": 0, "s": 18, "t": "READY", "d": ready}))?;
    let closed = arrival(
        ready.state,
        Arrival::Closed(Closure {
            code: None,
            reason: "network disconnected".into(),
        }),
    )?;
    assert!(closed.reconnect);
    let ReadStep::Socket(socket) =
        Plugin::read(identity()?, closed.state.clone()).map_err(|error| error.failure.message)?
    else {
        return Err("expected socket".into());
    };
    assert_eq!(
        socket.url,
        "wss://gateway-us.discord.gg/?v=10&encoding=json"
    );
    assert_eq!(socket.close_code, 4000);
    let resumed = frame(
        closed.state,
        json!({"op":10,"d":{"heartbeat_interval":1000}}),
    )?;
    let resume: Value =
        serde_json::from_str(&resumed.frames[0]).map_err(|error| error.to_string())?;
    assert_eq!(
        resume,
        json!({"op":6,"d":{"token":null,"session_id":"session","seq":18}})
    );
    let invalid = frame(resumed.state, json!({"op":9,"d":false}))?;
    assert!(
        invalid
            .timer
            .is_some_and(|wait| (1_000..=5_000).contains(&wait))
    );
    assert!(arrival(invalid.state.clone(), Arrival::Timer)?.reconnect);
    let hello = frame(
        invalid.state,
        json!({"op":10,"d":{"heartbeat_interval":1000}}),
    )?;
    assert!(hello.frames[0].contains(r#""op":2"#));
    let identity = identity()?;
    let rejected = |code, reason: &str| {
        let closure = Closure {
            code: Some(code),
            reason: reason.into(),
        };
        Plugin::received(identity.clone(), None, Arrival::Closed(closure))
    };
    let Err(intent) = rejected(4014, "Disallowed intent") else {
        return Err("expected a rejected intent".into());
    };
    assert!(intent.fatal && intent.failure.message.contains("Message Content intent"));
    assert!(rejected(4004, "Authentication failed").is_err_and(|error| error.fatal));
    Ok(())
}

#[test]
fn messages_keep_native_text_commands_addressing_threads_and_attachments() -> Result<(), String> {
    let mut plain = message();
    plain["content"] = json!("/cancel");
    let plain = created(1, plain)?;
    let inbound = &plain.messages[0];
    assert_eq!(inbound.key, "MESSAGE_CREATE:10");
    assert_eq!(inbound.conversation, "thread");
    assert!(!inbound.addressed);
    assert!(matches!(&inbound.action, Action::Message(content) if content.text == "/cancel"));
    let mut reply = message();
    reply["referenced_message"] =
        json!({"id": "8", "author": {"id": "42", "username": "Helper", "bot": true}});
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
        content.images[0].locator,
        "https://cdn.discordapp.com/attachments/a.webp"
    );
    assert_eq!(content.unviewable[0].what, "voice.ogg");
    let mut direct = message();
    direct
        .as_object_mut()
        .ok_or("message object")?
        .remove("guild_id");
    assert!(created(3, direct)?.messages[0].direct);
    let mut bot = message();
    bot["author"]["bot"] = json!(true);
    assert!(created(4, bot)?.messages.is_empty());
    Ok(())
}

#[test]
fn contentless_messages_are_not_input_and_forwards_are_not_replies() -> Result<(), String> {
    let mut sticker = message();
    sticker["content"] = json!("");
    sticker["sticker_items"] = json!([{"id": "1", "name": "wave", "format_type": 1}]);
    assert!(created(1, sticker)?.messages.is_empty());
    let forward = json!({"type": 1, "message_id": "8", "channel_id": "elsewhere"});
    let mut forwarded = message();
    forwarded["content"] = json!("");
    forwarded["message_reference"] = forward.clone();
    forwarded["message_snapshots"] = json!([{"message": {"content": "hello", "attachments": []}}]);
    assert!(created(2, forwarded)?.messages.is_empty());
    let mut annotated = message();
    annotated["message_reference"] = forward;
    assert_eq!(created(3, annotated)?.messages[0].reply_to, None);
    // A reply whose message was deleted still names it.
    let mut orphan = message();
    orphan["type"] = json!(19);
    orphan["referenced_message"] = Value::Null;
    orphan["message_reference"] = json!({"message_id": "8", "channel_id": "thread"});
    assert_eq!(
        created(4, orphan)?.messages[0].reply_to.as_deref(),
        Some("8")
    );
    Ok(())
}

#[test]
fn native_commands_and_buttons_defer_ephemerally_then_answer() -> Result<(), String> {
    let session = json!({"name": "session", "options": [{"name": "name", "value": "work"}]});
    let everyone = json!({"name": "everyone", "options": [{"name": "open", "value": "off"}]});
    for (kind, data, action) in [
        (2, json!({"name": "cancel"}), Action::Cancel),
        (3, json!({"custom_id": "cancel"}), Action::Cancel),
        (
            2,
            json!({"name": "Backtrack here", "target_id": "previous"}),
            Action::Backtrack,
        ),
        (2, session, Action::SelectSession("work".into())),
        (2, everyone, Action::Everyone(Some(false))),
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
fn split_and_components_keep_long_tables_files_and_text_within_limits() -> Result<(), String> {
    let text = format!(
        "{}\n---\n| Header | Value |\n|---|---|\n| A | B |\n\n```rust\nlet code = 1;\n```\n",
        "😀".repeat(4100)
    );
    let file = FileData {
        name: "plot.png".into(),
        data: b"\x89PNG\r\n\x1a\nplot".into(),
    };
    let parts = Plugin::split(Outgoing {
        text: text.clone(),
        file: Some(file.clone()),
    });
    assert!(parts.len() >= 3);
    assert_eq!(
        parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>(),
        text
    );
    assert_eq!(parts.iter().filter(|part| part.file.is_some()).count(), 1);
    for part in &parts {
        assert!(part.text.encode_utf16().count() <= 4000);
    }
    let first =
        Plugin::send("thread".into(), parts[0].clone()).map_err(|error| error.failure.message)?;
    assert_eq!(body(&first)?["flags"], 32768);
    let last = Plugin::send("thread".into(), parts.last().ok_or("last part")?.clone())
        .map_err(|error| error.failure.message)?;
    let bytes = last.body.ok_or("multipart")?;
    let encoded = String::from_utf8_lossy(&bytes);
    assert!(encoded.contains("payload_json"));
    assert!(encoded.contains("attachment://plot.png"));
    assert!(encoded.contains(r#""type":12"#));
    assert!(encoded.contains(r#""type":14"#));
    assert!(encoded.contains("```\\n| Header"));
    assert!(
        bytes
            .windows(file.data.len())
            .any(|bytes| bytes == file.data)
    );
    Ok(())
}

#[test]
fn split_preserves_markdown_and_budgets_the_rendered_components() -> Result<(), String> {
    for text in [
        "| h | v |\n| - | - |\n| a | b |\n\n".repeat(200),
        format!("```rust\n{}\n```\n", "let value = 1;\n".repeat(400)),
        "paragraph\n---\n".repeat(200),
    ] {
        let parts = Plugin::split(Outgoing {
            text: text.clone(),
            file: None,
        });
        assert_eq!(
            parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        for part in parts {
            let request =
                Plugin::send("thread".into(), part).map_err(|error| error.failure.message)?;
            let payload = body(&request)?;
            let components = payload["components"][0]["components"]
                .as_array()
                .ok_or("components")?;
            assert!(components.len() < 40);
            let units: usize = components
                .iter()
                .filter_map(|component| component["content"].as_str())
                .map(|text| text.encode_utf16().count())
                .sum();
            assert!(units <= 4000);
        }
    }
    Ok(())
}

#[test]
fn runs_show_typing_and_a_card_created_edited_and_deleted_with_cancel() -> Result<(), String> {
    let activity = ActivityView {
        elapsed_seconds: 125,
        tools: (2..7)
            .map(|index| ToolActivity {
                name: format!("tool-{index}"),
                state: ToolState::Running,
            })
            .collect(),
    };
    let created =
        Plugin::activity("thread".into(), None, Some(activity.clone())).ok_or("create")?;
    let body = body(&created)?;
    let content = body["components"][0]["components"][0]["content"]
        .as_str()
        .ok_or("text")?;
    assert!(content.starts_with("Working 2:05"));
    assert!(content.contains("… tool-2"));
    assert!(content.contains("… tool-6"));
    assert_eq!(
        body["components"][0]["components"][1]["components"][0]["custom_id"],
        "cancel"
    );
    assert_eq!(
        Plugin::activity("thread".into(), Some("card".into()), Some(activity))
            .ok_or("edit")?
            .method,
        "PATCH"
    );
    let deleted = Plugin::activity("thread".into(), Some("card".into()), None).ok_or("delete")?;
    assert_eq!(deleted.method, "DELETE");
    assert_eq!(deleted.url, "/channels/thread/messages/card");
    let typing = Plugin::presence("thread".into()).ok_or("typing")?;
    assert_eq!(
        (typing.method.as_str(), typing.url.as_str()),
        ("POST", "/channels/thread/typing")
    );
    Ok(())
}

#[test]
fn menus_have_native_contexts_and_http_errors_preserve_evidence() -> Result<(), String> {
    let menu = Plugin::commands(
        identity()?,
        vec![
            Command {
                kind: CommandKind::Session,
                description: "Choose a Session".into(),
                direct: true,
                group: false,
            },
            Command {
                kind: CommandKind::Backtrack,
                description: "Backtrack".into(),
                direct: true,
                group: true,
            },
        ],
    );
    assert_eq!(menu[0].url, "/applications/app/commands");
    let commands = body(&menu[0])?;
    assert_eq!(commands[0]["contexts"], json!([1]));
    assert_eq!(commands[1]["type"], 3);
    assert!(commands[1].get("description").is_none());
    assert_eq!(
        Plugin::answered(response(200, json!({"id":"sent"})))
            .map_err(|error| error.failure.message)?,
        Some("sent".into())
    );
    assert_eq!(
        Plugin::answered(response(200, json!([]))).map_err(|error| error.failure.message)?,
        None
    );
    assert!(
        matches!(Plugin::answered(response(429,json!({"retry_after":1.25}))),Err(error) if error.retry_after == Some(2) && !error.unknown && error.failure.retryable)
    );
    assert!(
        matches!(Plugin::answered(response(503,json!({}))),Err(error) if error.unknown && error.failure.retryable)
    );
    for malformed in [json!({}), json!({"id": 7}), json!(true)] {
        assert!(matches!(Plugin::answered(response(200, malformed)), Err(error) if error.unknown));
    }
    Ok(())
}
