//! Replies and forwards keep their original speakers, and a malformed part stays local.
use super::*;

fn receive_message(message: Value) -> Result<crate::channel::Inbound, String> {
    let body = json!({"ok": true, "result": [{"update_id": 300, "message": message}]}).to_string();
    Plugin::received(
        identity()?,
        None,
        Arrival::Response(response(200, body.as_bytes())),
    )
    .map_err(|error| error.failure.message)?
    .messages
    .into_iter()
    .next()
    .ok_or("missing message".into())
}

#[test]
fn forwarded_speech_keeps_all_origin_kinds_and_never_becomes_a_command() -> Result<(), String> {
    use enco_sdk::{channel::Quoted, types::QuoteKind};
    for (origin, sender, name, conversation) in [
        (
            json!({"type":"user","date":1793275200,"sender_user":{"id":80,"is_bot":false,"first_name":"Bob"}}),
            Some("80"),
            Some("Bob"),
            None,
        ),
        (
            json!({"type":"hidden_user","date":1793275200,"sender_user_name":"Hidden Bob"}),
            None,
            Some("Hidden Bob"),
            None,
        ),
        (
            json!({"type":"chat","date":1793275200,"sender_chat":{"id":-200,"type":"supergroup","title":"Trip group"},"author_signature":"Carol"}),
            None,
            Some("Carol"),
            Some("Trip group"),
        ),
        (
            json!({"type":"channel","date":1793275200,"chat":{"id":-300,"type":"channel","title":"Trip news"},"message_id":17}),
            None,
            None,
            Some("Trip news"),
        ),
    ] {
        let inbound = receive_message(json!({
            "message_id":10,"date":1793361600,"chat":{"id":-100,"type":"supergroup"},
            "from":{"id":7,"is_bot":false,"first_name":"Ada"},
            "forward_origin":origin,"text":"/cancel @helperbot",
            "entities":[{"type":"bot_command","offset":0,"length":7},{"type":"mention","offset":8,"length":10}]
        }))?;
        assert!(!inbound.addressed);
        let Action::Message(content) = inbound.action else {
            return Err("forward became command".into());
        };
        assert!(content.text.is_empty() && content.images.is_empty());
        let quote = content.quote.ok_or("missing quote")?;
        assert_eq!(quote.kind, QuoteKind::Forward);
        let Quoted::Messages(messages) = quote.messages else {
            return Err("missing messages".into());
        };
        assert_eq!(messages[0].text, "/cancel @helperbot");
        assert_eq!(messages[0].sender.as_deref(), sender);
        assert_eq!(messages[0].sender_name.as_deref(), name);
        assert_eq!(messages[0].conversation_name.as_deref(), conversation);
        assert_eq!(
            messages[0].sent_at.as_ref().map(|time| time.seconds),
            Some(1793275200)
        );
    }
    Ok(())
}

#[test]
fn replies_share_image_decoding_and_strip_only_leading_bot_mentions() -> Result<(), String> {
    use enco_sdk::{channel::Quoted, types::QuoteKind};
    let inbound = receive_message(json!({
        "message_id":11,"date":1793361600,"chat":{"id":-100,"type":"supergroup"},
        "from":{"id":7,"first_name":"Ada"},"text":"@helperbot @helperbot what about @helperbot?",
        "entities":[{"type":"mention","offset":0,"length":10},{"type":"mention","offset":11,"length":10},
            {"type":"mention","offset":33,"length":10}],
        "reply_to_message":{"message_id":9,"date":1793275200,"chat":{"id":-100,"type":"supergroup"},
            "from":{"id":80,"first_name":"Bob"},"caption":"@helperbot a photo",
            "caption_entities":[{"type":"mention","offset":0,"length":10}],
            "photo":[{"file_id":"small","file_unique_id":"small-unique","width":90,"height":90},
                {"file_id":"large","file_unique_id":"large-unique","width":640,"height":480},
                {"file_unique_id":"broken","width":1280,"height":960}]}
    }))?;
    assert!(inbound.addressed);
    assert_eq!(inbound.reply_to.as_deref(), Some("9"));
    let Action::Message(content) = inbound.action else {
        return Err("missing content".into());
    };
    assert_eq!(content.text, "what about @helperbot?");
    let quote = content.quote.ok_or("missing quote")?;
    assert_eq!(quote.kind, QuoteKind::Reply);
    let Quoted::Messages(messages) = quote.messages else {
        return Err("missing messages".into());
    };
    assert_eq!(messages[0].text, "@helperbot a photo");
    // The sizes are one picture: a malformed size is not shown missing while another works.
    assert_eq!(messages[0].images[0].locator, "large");
    assert!(messages[0].unviewable.is_empty());
    Ok(())
}

#[test]
fn malformed_content_is_visible_without_blocking_the_message() -> Result<(), String> {
    let message = json!({"message_id":12,"date":1793361600,"chat":{"id":9,"type":"private"},
        "from":{"id":7,"first_name":"Ada"},"caption":"keep this",
        "caption_entities":[{"type":"bold","length":4}],"document":{"file_name":"broken.png"},
        "reply_to_message":{"message_id":9,"date":1793275200,"chat":{"id":9,"type":"private"},
            "from":{"id":80,"first_name":"Bob"},"caption":"quoted","photo":[{"width":10}]}});
    let inbound = receive_message(message.clone())?;
    let Action::Message(content) = inbound.action else {
        return Err("missing content".into());
    };
    assert_eq!(content.text, "keep this");
    // A malformed bold entity changes nothing the text says, so only the file is missing.
    assert_eq!(content.unviewable.len(), 1);
    assert!(
        matches!(&content.unviewable[0].reason, enco_sdk::types::UnviewableReason::Unavailable(reason) if reason.contains("file_id"))
    );
    let enco_sdk::channel::Quoted::Messages(messages) =
        content.quote.ok_or("missing quote")?.messages
    else {
        return Err("missing quoted content".into());
    };
    assert_eq!(messages[0].text, "quoted");
    assert_eq!(messages[0].unviewable.len(), 1);
    let mut invalid_identity = message;
    invalid_identity["from"]["id"] = json!("not an ID");
    assert!(receive_message(invalid_identity).is_err());
    Ok(())
}

#[test]
fn replying_to_a_forward_uses_the_original_speaker() -> Result<(), String> {
    let inbound = receive_message(json!({
        "message_id":13,"date":1793361600,"chat":{"id":9,"type":"private"},
        "from":{"id":7,"first_name":"Ada"},"text":"What does this mean?",
        "reply_to_message":{"message_id":12,"date":1793361500,"chat":{"id":9,"type":"private"},
            "from":{"id":7,"first_name":"Ada"},"text":"/cancel",
            "forward_origin":{"type":"user","date":1793275200,"sender_user":{"id":80,"first_name":"Bob"}}}
    }))?;
    let Action::Message(content) = inbound.action else {
        return Err("missing content".into());
    };
    let enco_sdk::channel::Quoted::Messages(messages) =
        content.quote.ok_or("missing quote")?.messages
    else {
        return Err("missing messages".into());
    };
    assert_eq!(messages[0].sender.as_deref(), Some("80"));
    assert_eq!(messages[0].sender_name.as_deref(), Some("Bob"));
    assert_eq!(messages[0].text, "/cancel");
    assert_eq!(
        messages[0].sent_at.as_ref().map(|time| time.seconds),
        Some(1793275200)
    );
    Ok(())
}

#[test]
fn a_private_photo_forward_has_only_quoted_content() -> Result<(), String> {
    let inbound = receive_message(json!({
        "message_id":14,"date":1793361600,"chat":{"id":9,"type":"private"},
        "from":{"id":7,"first_name":"Ada"},"caption":"Trip map",
        "photo":[{"file_id":"quoted-image","file_unique_id":"unique","width":640,"height":480}],
        "forward_origin":{"type":"channel","date":1793275200,"chat":{"id":-300,"type":"channel","title":"Trip news"},"message_id":17}
    }))?;
    assert!(inbound.direct && inbound.addressed);
    let Action::Message(content) = inbound.action else {
        return Err("missing content".into());
    };
    assert!(content.text.is_empty() && content.images.is_empty());
    let enco_sdk::channel::Quoted::Messages(messages) =
        content.quote.ok_or("missing quote")?.messages
    else {
        return Err("missing messages".into());
    };
    assert_eq!(messages[0].text, "Trip map");
    assert_eq!(messages[0].images[0].locator, "quoted-image");
    Ok(())
}

#[test]
fn a_reply_to_an_anonymous_admin_is_by_the_group_it_was_sent_as() -> Result<(), String> {
    let inbound = receive_message(json!({
        "message_id":15,"date":1793361600,"chat":{"id":-100,"type":"supergroup","title":"Trip group"},
        "from":{"id":7,"first_name":"Ada"},"text":"Which day?",
        "reply_to_message":{"message_id":14,"date":1793275200,
            "chat":{"id":-100,"type":"supergroup","title":"Trip group"},
            "from":{"id":1087968824,"is_bot":true,"first_name":"Group","username":"GroupAnonymousBot"},
            "sender_chat":{"id":-100,"type":"supergroup","title":"Trip group"},"text":"Trip on Saturday"}
    }))?;
    let Action::Message(content) = inbound.action else {
        return Err("missing content".into());
    };
    let enco_sdk::channel::Quoted::Messages(messages) =
        content.quote.ok_or("missing quote")?.messages
    else {
        return Err("missing messages".into());
    };
    assert_eq!(messages[0].sender.as_deref(), Some("-100"));
    assert_eq!(messages[0].sender_name.as_deref(), Some("Trip group"));
    assert_eq!(messages[0].conversation_name, None);
    Ok(())
}

#[test]
fn a_malformed_forward_origin_keeps_the_forwarded_content() -> Result<(), String> {
    let inbound = receive_message(json!({
        "message_id":16,"date":1793361600,"chat":{"id":9,"type":"private"},
        "from":{"id":7,"first_name":"Ada"},"caption":"Trip map",
        "photo":[{"file_id":"map","file_unique_id":"map-unique","width":640,"height":480}],
        "forward_origin":{"type":"user","date":1793275200}
    }))?;
    let Action::Message(content) = inbound.action else {
        return Err("missing content".into());
    };
    let enco_sdk::channel::Quoted::Messages(messages) =
        content.quote.ok_or("missing quote")?.messages
    else {
        return Err("missing messages".into());
    };
    assert_eq!(messages[0].text, "Trip map");
    assert_eq!(messages[0].images[0].locator, "map");
    assert!(messages[0].sender.is_none() && messages[0].sender_name.is_none());
    assert!(messages[0].unviewable.is_empty());
    Ok(())
}
