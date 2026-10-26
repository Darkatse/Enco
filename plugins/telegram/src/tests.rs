use super::{
    Plugin,
    channel::{
        Action, Command, CommandKind, FetchStep, FileData, Guest, Identity, Outgoing, Response,
    },
};
use serde_json::{Value, json};

fn response(status: u16, body: &[u8]) -> Response {
    Response {
        status,
        headers: vec![],
        body: body.into(),
    }
}

fn identity() -> Result<Identity, String> {
    Plugin::identified(response(200, super::fixtures::IDENTITY.as_bytes()))
        .map_err(|fault| fault.failure.message)
}

#[test]
fn updates_translate_group_addressing_senders_and_attachment_locators() -> Result<(), String> {
    let updates = Plugin::received(
        identity()?,
        Some(r#"{"offset":100}"#.into()),
        response(200, super::fixtures::UPDATES.as_bytes()),
    )
    .map_err(|fault| fault.failure.message)?;
    assert_eq!(updates.len(), 9);
    let messages: Vec<_> = updates
        .iter()
        .filter_map(|update| update.message.as_ref())
        .collect();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.addressed)
            .collect::<Vec<_>>(),
        [false, true, true, false, true, true, false, false, false]
    );
    assert_eq!(messages[0].sender.as_deref(), Some("7"));
    assert_eq!(messages[0].sender_name.as_deref(), Some("Ada Lovelace"));
    assert!(matches!(messages[2].action, Action::Everyone(Some(true))));
    assert!(matches!(messages[3].action, Action::Message(_)));
    assert_eq!(messages[4].reply_to.as_deref(), Some("9"));
    let Action::Message(photo) = &messages[6].action else {
        return Err("expected photo".into());
    };
    assert_eq!(photo.images[0].locator, "large");
    let Action::Message(document) = &messages[7].action else {
        return Err("expected document".into());
    };
    assert_eq!(document.images[0].locator, "doc");
    let Action::Message(voice) = &messages[8].action else {
        return Err("expected voice".into());
    };
    assert_eq!(voice.unviewable[0].what, "voice message");
    let state = updates.last().map(|update| update.state.clone());
    let poll = Plugin::poll(identity()?, state).map_err(|fault| fault.failure.message)?;
    let body: Value = serde_json::from_slice(&poll.body.ok_or("missing poll body")?)
        .map_err(|error| error.to_string())?;
    assert_eq!(body["offset"], 109);
    Ok(())
}

#[test]
fn fetch_drives_two_requests_and_keeps_permanent_file_rejection_local() -> Result<(), String> {
    let FetchStep::Request(first) =
        Plugin::fetch("image-id".into(), vec![]).map_err(|fault| fault.failure.message)?
    else {
        return Err("expected getFile".into());
    };
    assert_eq!(first.path, "/bot/getFile");
    let metadata = response(
        200,
        br#"{"ok":true,"result":{"file_path":"photos/a b.jpg"}}"#,
    );
    let FetchStep::Request(download) = Plugin::fetch("image-id".into(), vec![metadata.clone()])
        .map_err(|fault| fault.failure.message)?
    else {
        return Err("expected download".into());
    };
    assert_eq!(download.path, "/file/bot/photos/a%20b.jpg");
    assert_eq!(
        Plugin::fetch(
            "image-id".into(),
            vec![metadata, response(200, b"image bytes")]
        )
        .map_err(|fault| fault.failure.message)?,
        FetchStep::Image(b"image bytes".to_vec())
    );
    assert!(matches!(
        Plugin::fetch("missing".into(), vec![response(400, b"not JSON")]),
        Ok(FetchStep::Unavailable(_))
    ));
    Ok(())
}

#[test]
fn protocol_faults_preserve_delivery_evidence_and_retry_delay() {
    assert!(
        matches!(Plugin::answered(response(200, br#"{"ok":false}"#)), Err(fault) if fault.failure.code == "channel.bad_response")
    );
    assert_eq!(
        Plugin::answered(response(200, br#"{"ok":true,"result":{"message_id":17}}"#)),
        Ok(Some("17".into()))
    );
    assert_eq!(
        Plugin::answered(response(200, br#"{"ok":true,"result":true}"#)),
        Ok(None)
    );
    for (status, unknown, fatal, retryable) in [
        (400, false, false, false),
        (401, false, true, false),
        (404, false, true, false),
        (429, false, false, true),
        (503, true, false, true),
        (200, true, false, false),
    ] {
        let result = Plugin::answered(response(status, b"invalid envelope"));
        assert!(
            matches!(result, Err(fault) if fault.unknown == unknown && fault.fatal == fatal && fault.failure.retryable == retryable)
        );
    }
    assert!(
        matches!(Plugin::answered(response(429, br#"{"ok":false,"error_code":429,"parameters":{"retry_after":17}}"#)), Err(fault) if fault.retry_after == Some(17))
    );
    assert!(
        matches!(Plugin::received(Identity { protocol: "telegram".into(), account: "42".into(), facts: r#"{"username":"HelperBot"}"#.into() }, None, response(200, br#"{"ok":true,"result":[{"update_id":"changed"}]}"#)), Err(fault) if fault.failure.code == "channel.bad_response")
    );
}

#[test]
fn private_commands_and_group_commands_target_only_this_bot() -> Result<(), String> {
    let cases = [
        ("private", "/session work", true),
        ("supergroup", "/cancel@helperbot", true),
        ("supergroup", "/backtrack", true),
        ("supergroup", "/everyone off", true),
        ("supergroup", "/cancel@OtherBot", false),
    ];
    for (kind, text, addressed) in cases {
        let body = json!({"ok":true,"result":[{"update_id":5,"message":{"message_id":1,"chat":{"id":9,"type":kind},"from":{"id":7,"first_name":"Ada"},"text":text}}]}).to_string();
        let updates = Plugin::received(identity()?, None, response(200, body.as_bytes()))
            .map_err(|fault| fault.failure.message)?;
        let message = updates[0].message.as_ref().ok_or("missing message")?;
        assert_eq!(message.addressed, addressed);
        match text {
            "/session work" => {
                assert!(matches!(&message.action, Action::SelectSession(name) if name == "work"))
            }
            "/cancel@helperbot" => assert!(matches!(message.action, Action::Cancel)),
            "/backtrack" => assert!(matches!(message.action, Action::Backtrack)),
            "/everyone off" => assert!(matches!(message.action, Action::Everyone(Some(false)))),
            _ => assert!(matches!(message.action, Action::Message(_))),
        }
    }
    Ok(())
}

#[test]
fn a_caption_is_never_a_command() -> Result<(), String> {
    let body = json!({"ok":true,"result":[{"update_id":5,"message":{"message_id":1,"chat":{"id":-100,"type":"supergroup"},"from":{"id":7,"first_name":"Ada"},"photo":[{"file_id":"photo","width":1,"height":1}],"caption":"/cancel"}}]}).to_string();
    let updates = Plugin::received(identity()?, None, response(200, body.as_bytes()))
        .map_err(|fault| fault.failure.message)?;
    let message = updates[0].message.as_ref().ok_or("missing message")?;
    assert!(!message.addressed);
    assert!(matches!(&message.action, Action::Message(content) if content.text == "/cancel"));
    Ok(())
}

#[test]
fn split_preserves_unicode_and_upload_keeps_file_bytes_outside_json() -> Result<(), String> {
    let text = format!("{}\n{}", "😀".repeat(32760), "next".repeat(40));
    let file = FileData {
        name: "song.mp3".into(),
        data: b"binary\0enco-telegram".into(),
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
    assert!(parts.iter().all(|part| part.text.chars().count() <= 32768));
    assert_eq!(parts.iter().filter(|part| part.file.is_some()).count(), 1);
    let part = parts.last().ok_or("missing upload")?.clone();
    let request = Plugin::send("-100".into(), part).map_err(|fault| fault.failure.message)?;
    let body = request.body.ok_or("missing upload body")?;
    assert!(
        body.windows(file.data.len())
            .any(|window| window == file.data)
    );
    let wire = String::from_utf8_lossy(&body);
    assert!(wire.contains(r#""type":"audio""#));
    assert!(wire.contains("filename=\"song.mp3\""));
    assert!(wire.contains("attach://file"));
    let boundary = request
        .headers
        .iter()
        .find_map(|(_, value)| value.strip_prefix("multipart/form-data; boundary="))
        .ok_or("missing multipart boundary")?;
    assert!(
        !file
            .data
            .windows(boundary.len())
            .any(|window| window == boundary.as_bytes())
    );
    Ok(())
}

#[test]
fn menus_use_chat_scopes_and_typing_uses_chat_action() -> Result<(), String> {
    let requests = Plugin::commands(vec![
        Command {
            kind: CommandKind::Session,
            description: "Choose Session".into(),
            direct: true,
            group: false,
        },
        Command {
            kind: CommandKind::Everyone,
            description: "Open group".into(),
            direct: false,
            group: true,
        },
    ]);
    let bodies: Vec<Value> = requests
        .iter()
        .map(|request| {
            serde_json::from_slice(request.body.as_deref().ok_or("missing menu body")?)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<_, _>>()?;
    assert_eq!(bodies[0]["scope"]["type"], "all_private_chats");
    assert_eq!(bodies[0]["commands"][0]["command"], "session");
    assert_eq!(bodies[1]["scope"]["type"], "all_group_chats");
    assert_eq!(bodies[1]["commands"][0]["command"], "everyone");
    let typing = Plugin::typing("-100".into()).ok_or("missing typing request")?;
    let body: Value = serde_json::from_slice(&typing.body.ok_or("missing typing body")?)
        .map_err(|error| error.to_string())?;
    assert_eq!(body, json!({"chat_id":"-100","action":"typing"}));
    Ok(())
}
