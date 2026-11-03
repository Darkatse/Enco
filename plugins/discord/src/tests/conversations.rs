use super::super::channel::FetchStep;
use super::*;
use enco_sdk::channel::{ActivityEpilogue, RunEnd};

#[test]
fn channel_names_use_authenticated_reads_and_permanent_errors_stay_local() -> Result<(), String> {
    let FetchStep::Request(request) =
        Plugin::fetch("channel:123".into(), vec![], 100).map_err(|f| f.failure.message)?
    else {
        return Err("request".into());
    };
    assert_eq!(
        (request.method.as_str(), request.url.as_str()),
        ("GET", "/channels/123")
    );
    let named = Plugin::fetch(
        "channel:123".into(),
        vec![response(200, json!({"id": "123", "name": "research"}))],
        100,
    )
    .map_err(|f| f.failure.message)?;
    assert!(matches!(named, FetchStep::Name(name) if name == "research"));
    let missing = Plugin::fetch(
        "channel:123".into(),
        vec![response(
            404,
            json!({"code":10003, "message":"Unknown Channel"}),
        )],
        100,
    )
    .map_err(|f| f.failure.message)?;
    assert!(matches!(missing, FetchStep::Unavailable(_)));
    for (status, body) in [
        (401, json!({"code": 0, "message": "401: Unauthorized"})),
        (429, json!({"retry_after": 2.0})),
        (503, json!({})),
    ] {
        assert!(Plugin::fetch("channel:123".into(), vec![response(status, body)], 100).is_err());
    }
    let download = Plugin::fetch("https://cdn.discordapp.com/picture.png".into(), vec![], 100)
        .map_err(|f| f.failure.message)?;
    assert!(matches!(download, FetchStep::Download(_)));
    Ok(())
}

#[test]
fn child_conversations_are_public_threads_and_archive_locked_on_close() -> Result<(), String> {
    let open = Plugin::open_conversation("123".into(), vec!["main".into(), "research".into()])
        .ok_or("open")?;
    assert_eq!(
        (open.method.as_str(), open.url.as_str()),
        ("POST", "/channels/123/threads")
    );
    assert_eq!(body(&open)?, json!({"name": "main › research", "type": 11}));
    let id = Plugin::opened(
        "123".into(),
        response(
            200,
            json!({"id":"456", "name":"main › research", "type":11}),
        ),
    )
    .map_err(|f| f.failure.message)?;
    assert_eq!(id, "456");
    let close = Plugin::close_conversation(id).ok_or("close")?;
    assert_eq!(
        (close.method.as_str(), close.url.as_str()),
        ("PATCH", "/channels/456")
    );
    assert_eq!(body(&close)?, json!({"archived":true, "locked":true}));
    let rejected = Plugin::opened(
        "123".into(),
        response(400, json!({"code":50035, "message":"Invalid Form Body"})),
    )
    .err()
    .ok_or("rejected")?;
    assert!(!rejected.unknown && !rejected.fatal && !rejected.failure.retryable);
    Ok(())
}

#[test]
fn terminal_views_leave_only_a_literal_text_component_without_cancel() -> Result<(), String> {
    let epilogue = ActivityEpilogue {
        elapsed_seconds: 420,
        rounds: 6,
        end: RunEnd::Completed,
        unsent: false,
        excerpt: "[three]* papers".into(),
    };
    for shown in [None, Some("8".into())] {
        let request = Plugin::activity(
            "123".into(),
            false,
            shown.clone(),
            ActivityUpdate::Epilogue(epilogue.clone()),
        )
        .ok_or("epilogue")?;
        let payload = body(&request)?;
        let components = payload["components"][0]["components"]
            .as_array()
            .ok_or("components")?;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0]["type"], 10);
        assert!(
            components[0]["content"]
                .as_str()
                .ok_or("text")?
                .contains(r"\[three]\* papers")
        );
        assert_eq!(
            payload["flags"],
            if shown.is_some() {
                32768
            } else {
                32768 | (1 << 12)
            }
        );
        assert_eq!(
            request.method,
            if shown.is_some() { "PATCH" } else { "POST" }
        );
    }
    Ok(())
}

#[test]
fn only_an_unknown_channel_removes_a_conversation() -> Result<(), String> {
    for (body, expected) in [
        (
            json!({"code": 10003, "message": "Unknown Channel"}).to_string(),
            "channel.conversation_gone",
        ),
        (
            json!({"code": 10008, "message": "Unknown Message"}).to_string(),
            "channel.http.404",
        ),
        ("<html>Not Found</html>".into(), "channel.http.404"),
    ] {
        let fault = Plugin::answered(Response {
            status: 404,
            headers: vec![],
            body: body.into_bytes(),
        })
        .err()
        .ok_or("fault")?;
        assert_eq!(fault.failure.code, expected);
        assert!(!fault.unknown && !fault.fatal && !fault.failure.retryable);
    }
    Ok(())
}
