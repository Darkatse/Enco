use super::*;
use enco_sdk::types::QuoteKind;

#[test]
fn replies_translate_mentions_emoji_and_quoted_images() -> Result<(), String> {
    let mut value = message();
    value["type"] = json!(19);
    value["content"] =
        json!("<@42> ask <@!81> <:wave:123> <a:dance:124> and <@42> <#82> <@&83> <3 <@81>");
    value["mentions"] = json!([
        {"id":"42","username":"helper","global_name":"Helper","bot":true},
        {"id":"81","username":"bob","global_name":"Bob"}
    ]);
    value["message_reference"] = json!({"type":0,"message_id":"8","channel_id":"thread"});
    value["referenced_message"] = json!({
        "id":"8","channel_id":"thread","type":0,
        "author":{"id":"81","username":"bob","global_name":"Bob"},
        "content":"<@42> look <:wave:123>","timestamp":"2026-10-29T14:00:00.123456+02:00",
        "mentions":[{"id":"42","username":"helper","global_name":"Helper","bot":true}],
        "attachments":[{"id":"90","filename":"trip.png","size":123,"url":"https://cdn.discordapp.com/attachments/trip.png","content_type":"image/png"}],
        "embeds":[],"components":[]
    });
    let reading = created(1, value)?;
    let inbound = &reading.messages[0];
    assert!(inbound.addressed);
    let Action::Message(content) = &inbound.action else {
        return Err("missing content".into());
    };
    assert_eq!(
        content.text,
        "ask @Bob :wave: :dance: and @Helper <#82> <@&83> <3 @Bob"
    );
    let quote = content.quote.as_ref().ok_or("missing quote")?;
    assert_eq!(quote.kind, QuoteKind::Reply);
    let Quoted::Messages(messages) = &quote.messages else {
        return Err("missing messages".into());
    };
    assert_eq!(messages[0].sender.as_deref(), Some("81"));
    // Only the incoming message drops its leading mention of this bot.
    assert_eq!(messages[0].text, "@Helper look :wave:");
    assert_eq!(
        messages[0].images[0].locator,
        "https://cdn.discordapp.com/attachments/trip.png"
    );
    assert_eq!(
        messages[0]
            .sent_at
            .as_ref()
            .map(|time| (time.seconds, time.nanoseconds)),
        Some((1793275200, 123456000))
    );
    Ok(())
}

#[test]
fn snapshots_and_replies_to_forwards_keep_unknown_senders() -> Result<(), String> {
    let forwarded = json!({
        "id":"8","channel_id":"elsewhere","type":0,"author":{"id":"81","username":"bob"},
        "content":"","timestamp":"2026-10-29T12:05:00.000000+00:00","mentions":[],"attachments":[],
        "embeds":[],"components":[],
        "message_reference":{"type":1,"message_id":"5","channel_id":"source"},
        "message_snapshots":[{"message":{
            "type":0,"content":"Forwarded text","timestamp":"2026-10-29T12:00:00.000000+00:00",
            "attachments":[],"mentions":[],"embeds":[],"components":[],"flags":0
        }}]
    });
    for reply in [false, true] {
        let mut value = message();
        value["content"] = json!("");
        value.as_object_mut().ok_or("message")?.remove("guild_id");
        if reply {
            value["type"] = json!(19);
            value["message_reference"] = json!({"type":0,"message_id":"8","channel_id":"thread"});
            value["referenced_message"] = forwarded.clone();
        } else {
            value["message_reference"] = forwarded["message_reference"].clone();
            value["message_snapshots"] = forwarded["message_snapshots"].clone();
        }
        let reading = created(1, value)?;
        let Action::Message(content) = &reading.messages[0].action else {
            return Err("missing content".into());
        };
        let quote = content.quote.as_ref().ok_or("missing quote")?;
        assert_eq!(
            quote.kind,
            if reply {
                QuoteKind::Reply
            } else {
                QuoteKind::Forward
            }
        );
        let Quoted::Messages(messages) = &quote.messages else {
            return Err("missing messages".into());
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].depth, 0);
        assert_eq!(messages[0].text, "Forwarded text");
        assert!(
            messages
                .iter()
                .all(|message| message.sender.is_none() && message.sender_name.is_none())
        );
    }
    Ok(())
}

#[test]
fn deleted_replies_and_broken_content_do_not_fail_message_identity() -> Result<(), String> {
    let mut value = message();
    value["type"] = json!(19);
    value["message_reference"] = json!({"message_id":"8","channel_id":"thread"});
    value["referenced_message"] = Value::Null;
    value["attachments"] = json!([
        {"filename":"bad.png","content_type":"image/png"},
        {"filename":"good.png","content_type":"image/png","url":"https://cdn.discordapp.com/attachments/good.png"}
    ]);
    let reading = created(1, value.clone())?;
    assert_eq!(reading.messages[0].reply_to.as_deref(), Some("8"));
    let Action::Message(content) = &reading.messages[0].action else {
        return Err("missing content".into());
    };
    assert_eq!(content.images.len(), 1);
    assert_eq!(content.unviewable.len(), 1);
    assert!(
        matches!(&content.quote.as_ref().ok_or("missing quote")?.messages, Quoted::Unavailable(reason) if reason.contains("deleted"))
    );
    value["author"]["id"] = json!(123);
    assert!(created(1, value).is_err());
    Ok(())
}
