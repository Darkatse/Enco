//! Updates, addressing and attachments translated at the protocol boundary.
use crate::{
    Facts,
    channel::{Fault, Identity, Inbound, Reading, Response},
    wire::{answer, conversation_id, invalid_response},
};
use enco_sdk::channel::{Action, CommandKind, ConversationName};
use enco_sdk::types::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The protocol state: where reading resumes, and the titles of topics not yet handed over.
#[derive(Default, Deserialize, Serialize)]
struct ReadState {
    /// The ID of the next update to read.
    offset: i64,
    /// Topic titles by conversation, kept from the service messages that gave them until a
    /// message from the topic hands them over.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    topics: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<Message>,
    callback_query: Option<Callback>,
    stopped_message_generation: Option<Stopped>,
}
#[derive(Deserialize)]
struct Stopped {
    chat: Chat,
    message_thread_id: Option<i64>,
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
    message_thread_id: Option<i64>,
    #[serde(default)]
    is_topic_message: bool,
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
    is_bot: bool,
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

impl Chat {
    /// Whether this is a private chat with a user, which only that user and the bot see.
    fn private(&self) -> bool {
        self.kind == "private"
    }

    /// The conversation of a message in this chat with `thread`, and the realm it is in: a topic
    /// is a conversation of its own in the chat's realm; elsewhere the chat is a conversation on
    /// its own.
    fn conversation(
        &self,
        thread: Option<i64>,
        is_topic_message: bool,
    ) -> (String, Option<String>) {
        // A private chat has no reply threads, so a thread there always names a topic.
        let topic = thread.filter(|_| self.private() || is_topic_message);
        (
            conversation_id(self.id, topic),
            topic.map(|_| self.id.to_string()),
        )
    }
}

/// The title of the topic `message` created, when it is the service message that created one.
///
/// That message is the topic's root; a message in the topic may carry it as `reply_to_message`.
pub(super) fn root_title(message: &Map<String, Value>) -> Option<&str> {
    message.get("forum_topic_created")?.get("name")?.as_str()
}

/// The title a service message gives its topic, by creating or renaming it.
fn service_title(body: &Map<String, Value>) -> Option<&str> {
    root_title(body).or_else(|| body.get("forum_topic_edited")?.get("name")?.as_str())
}

pub(super) fn offset(state: Option<&str>) -> Result<i64, Fault> {
    Ok(read_state(state)?.offset)
}

fn read_state(state: Option<&str>) -> Result<ReadState, Fault> {
    match state {
        None => Ok(ReadState::default()),
        Some(state) => serde_json::from_str::<ReadState>(state).map_err(|_| Fault {
            failure: Failure {
                code: "channel.invalid_state".into(),
                message: "invalid Telegram read state".into(),
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
    let mut state = read_state(state.as_deref())?;
    let facts = Facts::of(&identity)?;
    let updates: Vec<TelegramUpdate> = answer("getUpdates", response)?;
    let mut messages = Vec::new();
    for update in updates {
        let next = update
            .update_id
            .checked_add(1)
            .ok_or_else(|| invalid_response("getUpdates update_id"))?;
        if next <= state.offset {
            return Err(invalid_response("getUpdates update order"));
        }
        state.offset = next;
        let key = update.update_id.to_string();
        let message = if let Some(callback) = update.callback_query {
            callback_inbound(callback, key)?
        } else if let Some(stopped) = update.stopped_message_generation {
            Some(stopped_inbound(stopped, key))
        } else if let Some(message) = update.message {
            let topics = &mut state.topics;
            message_inbound(message, key, topics, &identity.account, &facts.username)
        } else {
            None
        };
        messages.extend(message);
    }
    Ok(Reading {
        messages,
        state: Some(json!(state).to_string()),
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
    let (conversation, realm) = message
        .chat
        .conversation(message.message_thread_id, message.is_topic_message);
    Ok(Some(Inbound {
        key,
        conversation,
        realm,
        conversation_name: None,
        message: message.message_id.to_string(),
        sender: Some(callback.from.id.to_string()),
        sender_name: Some(callback.from.name()),
        direct: message.chat.private(),
        addressed: true,
        reply_to: None,
        respond: Some(callback.id),
        action: Action::Cancel,
    }))
}

/// What `message` asks; absent for a bot's message and for one that shows nothing.
///
/// A service message's title waits in `topics` until a message from its topic hands it over.
fn message_inbound(
    message: Message,
    key: String,
    topics: &mut BTreeMap<String, String>,
    account: &str,
    username: &str,
) -> Option<Inbound> {
    if message.from.as_ref().is_some_and(|user| user.is_bot) {
        return None;
    }
    let (conversation, realm) = message
        .chat
        .conversation(message.message_thread_id, message.is_topic_message);
    if realm.is_some()
        && let Some(title) = service_title(&message.body)
    {
        topics.insert(conversation.clone(), title.into());
    }
    let direct = message.chat.private();
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
    // A title held from a service message is newer than the topic root's.
    let title = topics.remove(&conversation).or_else(|| {
        let replied = message.body.get("reply_to_message")?.as_object()?;
        root_title(replied).map(Into::into)
    });
    Some(Inbound {
        key,
        respond: None,
        conversation,
        realm,
        conversation_name: title.map(ConversationName::Name),
        message: message.message_id.to_string(),
        sender: message.from.as_ref().map(|user| user.id.to_string()),
        sender_name: message.from.as_ref().map(User::name),
        direct,
        addressed,
        reply_to: decoded.reply_to,
        action,
    })
}

/// The owner stopped a draft, which only a private chat has: its ID is the owner's.
///
/// A stop is no message of its own; its ID is the update's, marked so that it never equals a
/// message's.
fn stopped_inbound(stopped: Stopped, key: String) -> Inbound {
    let (conversation, realm) = stopped.chat.conversation(stopped.message_thread_id, false);
    Inbound {
        message: format!("stop:{key}"),
        key,
        conversation,
        realm,
        conversation_name: None,
        sender: Some(stopped.chat.id.to_string()),
        sender_name: None,
        direct: true,
        addressed: true,
        reply_to: None,
        respond: None,
        action: Action::Cancel,
    }
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
