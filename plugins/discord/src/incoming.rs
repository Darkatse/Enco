//! Discord messages and native interactions translated once at the external boundary.
use crate::{
    channel::{Command, Fault, Inbound, Request},
    wire,
};
use enco_sdk::channel::{Action, CommandKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The interaction message flag that shows a response only to the user who invoked it.
const EPHEMERAL: u32 = 1 << 6;
/// The message menu entry that backtracks; it acts on the message it is invoked on.
const BACKTRACK_HERE: &str = "Backtrack here";

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
    pub(super) fn name(&self) -> String {
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
    #[serde(flatten)]
    body: serde_json::Map<String, Value>,
    #[serde(rename = "type")]
    kind: u8,
}

pub(super) fn message(value: Value, account: &str) -> Result<Option<Inbound>, Fault> {
    let message: Message =
        serde_json::from_value(value).map_err(|_| wire::invalid("MESSAGE_CREATE"))?;
    if message.author.bot || !matches!(message.kind, 0 | 19) {
        return Ok(None);
    }
    let direct = message.guild_id.is_none();
    let decoded = crate::content::decode(&message.body, account);
    if decoded.content.is_empty() {
        return Ok(None);
    }
    Ok(Some(Inbound {
        key: format!("MESSAGE_CREATE:{}", message.id),
        conversation: message.channel_id,
        message: message.id,
        sender_name: Some(message.author.name()),
        sender: Some(message.author.id),
        direct,
        addressed: direct || decoded.addressed,
        reply_to: decoded.reply_to,
        respond: None,
        action: Action::Message(decoded.content),
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
        (2, Some(BACKTRACK_HERE), _) => Action::Backtrack,
        (2, Some(name), _) => {
            let Some(kind) = CommandKind::from_name(name) else {
                return Ok(None);
            };
            let argument = option(kind)
                .and_then(|(name, ..)| {
                    interaction
                        .data
                        .options
                        .iter()
                        .find(|value| value.name == name)
                })
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

/// The string option a slash command takes, as it is registered and as an interaction returns
/// it: name, description and fixed choices; absent for a command without an argument.
fn option(kind: CommandKind) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    Some(match kind {
        CommandKind::Session => ("name", "Session to switch to", &[]),
        CommandKind::Halt => ("name", "Session to halt", &[]),
        CommandKind::Archive => ("name", "Session to archive", &[]),
        CommandKind::Everyone => ("open", "Open the group to everyone", &["on", "off"]),
        CommandKind::Cancel | CommandKind::Backtrack => return None,
    })
}

fn command(command: Command) -> Value {
    let mut contexts = vec![];
    if command.group {
        contexts.push(0);
    }
    if command.direct {
        contexts.push(1);
    }
    let (name, kind) = match command.kind {
        CommandKind::Backtrack => (BACKTRACK_HERE, 3),
        CommandKind::Session
        | CommandKind::Cancel
        | CommandKind::Everyone
        | CommandKind::Halt
        | CommandKind::Archive => (command.kind.name(), 1),
    };
    let mut value = json!({
        "name": name,
        "type": kind,
        "contexts": contexts,
        "integration_types": [0],
    });
    if kind == 1 {
        value["description"] = json!(command.description);
        value["options"] = option(command.kind)
            .map(|(name, description, choices)| {
                let mut option = json!({
                    "type": 3,
                    "name": name,
                    "description": description,
                    "required": false,
                });
                if !choices.is_empty() {
                    option["choices"] = choices
                        .iter()
                        .map(|choice| json!({"name": choice, "value": choice}))
                        .collect();
                }
                option
            })
            .into_iter()
            .collect();
    }
    value
}
