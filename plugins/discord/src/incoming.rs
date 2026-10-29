//! Discord messages and native interactions translated once at the external boundary.
use crate::{
    channel::{Command, Fault, Inbound, Request},
    wire,
};
use enco_sdk::channel::{Action, Attachment, CommandKind, Content};
use enco_sdk::types::{Unviewable, UnviewableReason};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Discord's `DEFAULT` message reference type, which replies use; a forward's reference names the
/// message it copies, not one it answers.
const REPLY_REFERENCE: u8 = 0;
/// The interaction message flag that shows a response only to the user who invoked it.
const EPHEMERAL: u32 = 1 << 6;

#[derive(Deserialize)]
pub(super) struct User {
    pub id: String,
    username: String,
    global_name: Option<String>,
    #[serde(default)]
    bot: bool,
}

impl User {
    /// The name Discord shows for this user: the display name, or the username without one.
    fn name(&self) -> String {
        self.global_name
            .clone()
            .unwrap_or_else(|| self.username.clone())
    }
}

#[derive(Deserialize)]
struct Message {
    id: String,
    channel_id: String,
    guild_id: Option<String>,
    author: User,
    content: String,
    #[serde(default)]
    mentions: Vec<User>,
    #[serde(default)]
    attachments: Vec<File>,
    referenced_message: Option<Reference>,
    message_reference: Option<MessageReference>,
    #[serde(rename = "type")]
    kind: u8,
}

#[derive(Deserialize)]
struct Reference {
    id: String,
    author: User,
}
#[derive(Deserialize)]
struct MessageReference {
    // Discord omits the type for its default.
    #[serde(rename = "type", default)]
    kind: u8,
    message_id: Option<String>,
}
#[derive(Deserialize)]
struct File {
    filename: String,
    url: String,
    content_type: Option<String>,
}

pub(super) fn message(value: Value, account: &str) -> Result<Option<Inbound>, Fault> {
    let message: Message =
        serde_json::from_value(value).map_err(|_| wire::invalid("MESSAGE_CREATE"))?;
    if message.author.bot || !matches!(message.kind, 0 | 19) {
        return Ok(None);
    }
    let direct = message.guild_id.is_none();
    let addressed = direct
        || message.mentions.iter().any(|user| user.id == account)
        || message
            .referenced_message
            .as_ref()
            .is_some_and(|reply| reply.author.id == account);
    let reply_to = message
        .referenced_message
        .map(|reply| reply.id)
        .or_else(|| {
            message
                .message_reference
                .filter(|reference| reference.kind == REPLY_REFERENCE)
                .and_then(|reference| reference.message_id)
        });
    let mut content = Content {
        text: message.content,
        images: vec![],
        unviewable: vec![],
    };
    for file in message.attachments {
        if file
            .content_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            content.images.push(Attachment {
                what: file.filename,
                locator: file.url,
            });
        } else {
            content.unviewable.push(Unviewable {
                what: file.filename,
                reason: UnviewableReason::Unsupported,
            });
        }
    }
    if content.text.is_empty() && content.images.is_empty() && content.unviewable.is_empty() {
        return Ok(None);
    }
    Ok(Some(Inbound {
        key: format!("MESSAGE_CREATE:{}", message.id),
        conversation: message.channel_id,
        message: message.id,
        sender_name: Some(message.author.name()),
        sender: Some(message.author.id),
        direct,
        addressed,
        reply_to,
        respond: None,
        action: Action::Message(content),
    }))
}

#[derive(Deserialize)]
struct Interaction {
    id: String,
    application_id: String,
    token: String,
    channel_id: String,
    guild_id: Option<String>,
    member: Option<Member>,
    user: Option<User>,
    #[serde(rename = "type")]
    kind: u8,
    data: InteractionData,
}
#[derive(Deserialize)]
struct Member {
    user: User,
}
#[derive(Deserialize)]
struct InteractionData {
    name: Option<String>,
    custom_id: Option<String>,
    target_id: Option<String>,
    #[serde(default)]
    options: Vec<CommandOption>,
}
#[derive(Deserialize)]
struct CommandOption {
    name: String,
    value: String,
}

#[derive(Deserialize, Serialize)]
struct Handle {
    application: String,
    token: String,
}

pub(super) fn interaction(value: Value) -> Result<Option<(Inbound, Request)>, Fault> {
    let interaction: Interaction =
        serde_json::from_value(value).map_err(|_| wire::invalid("INTERACTION_CREATE"))?;
    let action = match (
        interaction.kind,
        interaction.data.name.as_deref(),
        interaction.data.custom_id.as_deref(),
    ) {
        (3, _, Some("cancel")) => Action::Cancel,
        (2, Some("Backtrack here"), _) => Action::Backtrack,
        (2, Some(name), _) => {
            let Some(kind) = CommandKind::from_name(name) else {
                return Ok(None);
            };
            let option = match kind {
                CommandKind::Session => "name",
                CommandKind::Everyone => "open",
                CommandKind::Cancel | CommandKind::Backtrack => "",
            };
            let argument = interaction
                .data
                .options
                .iter()
                .find(|value| value.name == option)
                .map_or("", |value| value.value.as_str());
            kind.action(argument)
                .ok_or_else(|| wire::invalid("command option"))?
        }
        _ => return Ok(None),
    };
    let user = interaction
        .member
        .map(|member| member.user)
        .or(interaction.user)
        .ok_or_else(|| wire::invalid("interaction sender"))?;
    let acknowledgment = wire::request(
        "POST",
        format!(
            "/interactions/{}/{}/callback",
            wire::path(&interaction.id),
            wire::path(&interaction.token)
        ),
        Some(json!({"type": 5, "data": {"flags": EPHEMERAL}})),
    );
    let handle = Handle {
        application: interaction.application_id,
        token: interaction.token,
    };
    Ok(Some((
        Inbound {
            key: format!("INTERACTION_CREATE:{}", interaction.id),
            conversation: interaction.channel_id,
            message: interaction.id,
            sender_name: Some(user.name()),
            sender: Some(user.id),
            direct: interaction.guild_id.is_none(),
            addressed: true,
            reply_to: interaction.data.target_id,
            respond: Some(json!(handle).to_string()),
            action,
        },
        acknowledgment,
    )))
}

pub(super) fn respond(handle: &str, text: Option<String>) -> Result<Option<Request>, Fault> {
    let handle: Handle = serde_json::from_str(handle)
        .map_err(|_| wire::fault("plugin.contract", "invalid Discord response handle", true))?;
    let url = format!(
        "/webhooks/{}/{}/messages/@original",
        wire::path(&handle.application),
        wire::path(&handle.token)
    );
    Ok(Some(match text {
        Some(text) => wire::request(
            "PATCH",
            url,
            Some(json!({"content": text, "allowed_mentions": {"parse": []}})),
        ),
        None => wire::request("DELETE", url, None),
    }))
}

pub(super) fn commands(commands: Vec<Command>) -> Vec<Value> {
    commands.into_iter().map(command).collect()
}

fn command(command: Command) -> Value {
    let mut contexts = vec![];
    if command.group {
        contexts.push(0);
    }
    if command.direct {
        contexts.push(1);
    }
    let (name, kind, options) = match command.kind {
        CommandKind::Session => {
            let name = json!({
                "type": 3,
                "name": "name",
                "description": "Session to switch to",
                "required": false,
            });
            (command.kind.name(), 1, json!([name]))
        }
        CommandKind::Cancel => (command.kind.name(), 1, json!([])),
        CommandKind::Backtrack => ("Backtrack here", 3, json!([])),
        CommandKind::Everyone => {
            let choices = json!([{"name": "on", "value": "on"}, {"name": "off", "value": "off"}]);
            let open = json!({
                "type": 3,
                "name": "open",
                "description": "Open the group to everyone",
                "required": false,
                "choices": choices,
            });
            (command.kind.name(), 1, json!([open]))
        }
    };
    let mut value = json!({
        "name": name,
        "type": kind,
        "contexts": contexts,
        "integration_types": [0],
    });
    if kind == 1 {
        value["description"] = json!(command.description);
        value["options"] = options;
    }
    value
}
