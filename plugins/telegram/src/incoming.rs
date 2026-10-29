//! Updates, addressing and attachments translated at the protocol boundary.
use crate::{
    channel::{Fault, Identity, Inbound, Reading, Response},
    wire::{answer, invalid_response},
};
use enco_sdk::channel::{Action, CommandKind};
use enco_sdk::types::Failure;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
struct OffsetState {
    offset: i64,
}
#[derive(Deserialize)]
struct Facts {
    username: String,
}
#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<Message>,
    callback_query: Option<Callback>,
}
#[derive(Deserialize)]
struct Callback {
    id: String,
    from: User,
    message: Option<Message>,
    data: Option<String>,
}
#[derive(Deserialize)]
struct Message {
    message_id: i64,
    chat: Chat,
    from: Option<User>,
    #[serde(flatten)]
    body: Map<String, Value>,
}
#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Deserialize)]
pub(super) struct User {
    pub(super) id: i64,
    first_name: String,
    last_name: Option<String>,
}

impl User {
    /// The name Telegram shows for this user: the first name, then the last name when there is one.
    pub(super) fn name(&self) -> String {
        match &self.last_name {
            Some(last) => format!("{} {last}", self.first_name),
            None => self.first_name.clone(),
        }
    }
}
pub(super) fn offset(state: Option<&str>) -> Result<i64, Fault> {
    match state {
        None => Ok(0),
        Some(state) => serde_json::from_str::<OffsetState>(state)
            .map(|state| state.offset)
            .map_err(|_| Fault {
                failure: Failure {
                    code: "channel.invalid_state".into(),
                    message: "invalid Telegram offset state".into(),
                    retryable: false,
                },
                unknown: false,
                fatal: true,
                retry_after: None,
            }),
    }
}

pub(super) fn received(
    identity: Identity,
    state: Option<String>,
    response: Response,
) -> Result<Reading, Fault> {
    let mut offset = offset(state.as_deref())?;
    let facts: Facts = serde_json::from_str(&identity.facts).map_err(|_| Fault {
        failure: Failure {
            code: "plugin.contract".into(),
            message: "identity facts are not the ones `identified` returns".into(),
            retryable: false,
        },
        unknown: false,
        fatal: true,
        retry_after: None,
    })?;
    let updates: Vec<TelegramUpdate> = answer("getUpdates", response)?;
    let mut messages = Vec::new();
    for update in updates {
        let next = update
            .update_id
            .checked_add(1)
            .ok_or_else(|| invalid_response("getUpdates update_id"))?;
        if next <= offset {
            return Err(invalid_response("getUpdates update order"));
        }
        offset = next;
        let key = update.update_id.to_string();
        let message = if let Some(callback) = update.callback_query {
            callback_inbound(callback, key)?
        } else {
            update
                .message
                .and_then(|message| inbound(message, key, &identity.account, &facts.username))
        };
        if let Some(message) = message {
            messages.push(message);
        }
    }
    Ok(Reading {
        messages,
        state: Some(json!({"offset": offset}).to_string()),
        frames: vec![],
        acknowledgments: vec![],
        timer: None,
        reconnect: false,
    })
}

fn callback_inbound(callback: Callback, key: String) -> Result<Option<Inbound>, Fault> {
    if callback.data.as_deref() != Some("cancel") {
        return Ok(None);
    }
    let message = callback
        .message
        .ok_or_else(|| invalid_response("callback_query message"))?;
    Ok(Some(Inbound {
        key,
        conversation: message.chat.id.to_string(),
        message: message.message_id.to_string(),
        sender: Some(callback.from.id.to_string()),
        sender_name: Some(callback.from.name()),
        direct: message.chat.kind == "private",
        addressed: true,
        reply_to: None,
        respond: Some(callback.id),
        action: Action::Cancel,
    }))
}

fn inbound(message: Message, key: String, account: &str, username: &str) -> Option<Inbound> {
    let direct = message.chat.kind == "private";
    let decoded = crate::content::decode(&message.body, account, username);
    let content = decoded.content;
    if content.is_empty() {
        return None;
    }
    // Only text alone is a command; a forward's text is the original sender's, so it has none.
    let command = (content.images.is_empty() && content.unviewable.is_empty())
        .then(|| command(&content.text, username))
        .flatten();
    let addressed = direct
        || match command {
            Some(CommandTarget::OtherBot) => false,
            Some(CommandTarget::ThisBot(_)) => true,
            None => decoded.addressed,
        };
    let action = match command {
        Some(CommandTarget::ThisBot(Some(action))) => action,
        _ => Action::Message(content),
    };
    Some(Inbound {
        key,
        respond: None,
        conversation: message.chat.id.to_string(),
        message: message.message_id.to_string(),
        sender: message.from.as_ref().map(|user| user.id.to_string()),
        sender_name: message.from.as_ref().map(User::name),
        direct,
        addressed,
        reply_to: decoded.reply_to,
        action,
    })
}

/// The bot a leading `/command` is for.
enum CommandTarget {
    /// Another bot, named after `@`.
    OtherBot,
    /// This bot, named after `@` or by one of its commands; the action when it knows the command
    /// and its argument.
    ThisBot(Option<Action>),
}

/// The bot `text` commands; absent when it starts with no command, or with one that names no bot
/// and that this bot does not understand.
fn command(text: &str, username: &str) -> Option<CommandTarget> {
    let (head, argument) = enco_sdk::channel::typed_command(text)?;
    let (name, target) = head
        .split_once('@')
        .map_or((head, None), |(name, target)| (name, Some(target)));
    if target.is_some_and(|target| !target.eq_ignore_ascii_case(username)) {
        return Some(CommandTarget::OtherBot);
    }
    let action = CommandKind::from_name(name).and_then(|kind| kind.action(argument));
    (action.is_some() || target.is_some()).then_some(CommandTarget::ThisBot(action))
}
