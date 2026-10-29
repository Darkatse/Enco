//! The connection's update boundary: routing, group context and owner commands commit here.
use super::*;
use enco_kernel::{ChannelCommand, ChannelCommandKind};
use transport::Disposition;

const NO_GROUP_PROFILE: &str =
    "Set group_profile for this connection to use the Agent in group chats.";
const GROUP_SESSION: &str = "A group chat always uses its own Session.";
const REFUSAL: &str = "Only the owner can use this.";

/// The commands the connection understands, as offered in the protocol's menus.
fn commands() -> Vec<ChannelCommand> {
    use ChannelCommandKind::{Backtrack, Cancel, Everyone, Session};
    [
        (
            Session,
            "List Sessions, or switch with /session <name>",
            true,
            false,
        ),
        (Cancel, "Stop the current work", true, true),
        (
            Backtrack,
            "Reply to a message with this to go back to it",
            true,
            true,
        ),
        (
            Everyone,
            "on or off: let everyone in this group ask the Agent",
            false,
            true,
        ),
    ]
    .into_iter()
    .map(|(kind, description, direct, group)| ChannelCommand {
        kind,
        description: description.into(),
        direct,
        group,
    })
    .collect()
}

impl Connection {
    /// Offer the commands in the protocol's menus; a failure is reported, not fatal.
    pub(super) async fn register_commands(&self) {
        if let Err(error) = health::call(
            &self.kernel,
            self.generation,
            self.adapter.commands(&commands()),
        )
        .await
        {
            tracing::warn!(%error, "channel command menu registration failed");
            *lock(&self.menu_failure) = Some(error.to_string());
        }
    }

    /// Accept one prepared message; its receipt waits for the reading's acknowledgments.
    pub(super) async fn accept(&mut self, update: transport::Prepared) -> Result<(), ChannelError> {
        let transport::Prepared {
            key,
            origin,
            disposition,
        } = update;
        let mut state = self.state.clone();
        let receipt = match disposition {
            Disposition::Duplicate => return Ok(()),
            Disposition::Refused { handle } => {
                self.commit_update(state, key, None, None).await?;
                Some(Receipt::Answer {
                    handle,
                    text: Some(REFUSAL.into()),
                })
            }
            Disposition::Ignored => {
                self.commit_update(state, key, None, None).await?;
                None
            }
            Disposition::Earlier {
                conversation,
                message,
            } => {
                if self.settings.context > 0 {
                    let buffer = state.context.entry(conversation).or_default();
                    buffer.push(message);
                    let excess = buffer.len().saturating_sub(self.settings.context);
                    buffer.drain(..excess);
                }
                self.commit_update(state, key, None, None).await?;
                None
            }
            Disposition::Input(input) => {
                let target = input.conversation.clone();
                let respond = input.respond.clone();
                let text = self.input(state, key, origin, input).await?;
                match respond {
                    Some(handle) => Some(Receipt::Answer { handle, text }),
                    None => text.map(|text| Receipt::Say { target, text }),
                }
            }
        };
        self.receipts.extend(receipt);
        Ok(())
    }

    /// Carry out an authorized input; the result is the receipt's text, if it has one.
    async fn input(
        &mut self,
        mut state: State,
        key: String,
        origin: MappingOrigin,
        input: transport::Input,
    ) -> Result<Option<String>, ChannelError> {
        let transport::Input {
            conversation: target,
            message,
            sender,
            sender_name,
            owner,
            direct,
            reply_to,
            respond: _,
            action,
        } = input;
        let source = EventSource::Channel {
            protocol: self.identity.protocol.clone(),
            account: self.identity.account.clone(),
            conversation: target.clone(),
            sender,
            sender_name,
            owner,
            message,
            origin,
        };
        let receipt = match action {
            Action::Message(content) => {
                return self
                    .message(state, key, source, (&target, direct), content)
                    .await;
            }
            Action::Backtrack => {
                return self.backtrack(state, &target, source, key, reply_to).await;
            }
            Action::SelectSession(_) | Action::Sessions if !direct => GROUP_SESSION.into(),
            Action::SelectSession(name) => {
                let session = self.kernel.open_session(&name).await?.id;
                state.chats.insert(target.clone(), session);
                format!("Switched to {name}.")
            }
            Action::Sessions => self.sessions(&target).await?,
            Action::Cancel => {
                // The cancellation follows the accepted command, never a command still unwritten.
                self.commit_update(state, key, None, None).await?;
                let result = self
                    .state
                    .chats
                    .get(&target)
                    .map(|session| self.kernel.cancel(*session))
                    .transpose();
                return Ok(Some(match result {
                    Ok(Some(true)) => "Cancellation requested.".into(),
                    Ok(_) => "No active Run.".into(),
                    Err(error) => format!("Cancel failed: {error}"),
                }));
            }
            Action::Everyone(open) => everyone(&mut state, &target, direct, open),
        };
        self.receipt(state, key, receipt).await
    }

    /// Route a message to its conversation's Session; a group's first creates the group's own.
    async fn message(
        &mut self,
        mut state: State,
        key: String,
        source: EventSource,
        (target, direct): (&str, bool),
        content: enco_kernel::ChannelContent<Image, Quote>,
    ) -> Result<Option<String>, ChannelError> {
        let group_profile = match (direct, &self.settings.group_profile) {
            (true, _) => None,
            (false, Some(profile)) => Some(profile.clone()),
            (false, None) => return self.receipt(state, key, NO_GROUP_PROFILE.into()).await,
        };
        let (session, created) = match (state.chats.get(target), group_profile) {
            (Some(session), _) => (*session, None),
            (None, None) => (self.kernel.open_session("main").await?.id, None),
            (None, Some(profile)) => {
                let created = NewSession {
                    id: SessionId::new(),
                    name: format!("{}:{target}", self.settings.name),
                    profile,
                    created_at: self.clock.now(),
                };
                (created.id, Some(created))
            }
        };
        state.chats.insert(target.into(), session);
        let earlier = state.context.remove(target).unwrap_or_default();
        let body = EventBody::UserMessage {
            text: content.text,
            images: content.images,
            unviewable: content.unviewable,
            earlier,
            quote: content.quote,
        };
        self.submit(state, session, source, body, key, created)
            .await?;
        Ok(None)
    }

    /// Accept the update; `text` is its receipt.
    pub(super) async fn receipt(
        &mut self,
        state: State,
        key: String,
        text: String,
    ) -> Result<Option<String>, ChannelError> {
        self.commit_update(state, key, None, None).await?;
        Ok(Some(text))
    }

    /// Accept the update with an input for `session`, creating it when `created` says so, and
    /// follow the Session's replies.
    pub(super) async fn submit(
        &mut self,
        mut state: State,
        session: SessionId,
        source: EventSource,
        body: EventBody,
        key: String,
        created: Option<NewSession>,
    ) -> Result<(), ChannelError> {
        state.outbound.entry(session).or_default();
        let event = Event {
            id: EventId::new(),
            session,
            source,
            body,
            received_at: self.clock.now(),
        };
        self.commit_update(state, key, Some(event), created).await?;
        self.follow(session)?;
        self.observe(session).await
    }
}

fn everyone(state: &mut State, target: &str, direct: bool, open: Option<bool>) -> String {
    if direct {
        return "Opening to everyone applies only to group chats.".into();
    }
    match open {
        Some(true) => {
            state.everyone.insert(target.into());
            "Everyone in this group can now ask the Agent."
        }
        Some(false) => {
            state.everyone.remove(target);
            "Only you can ask the Agent in this group now."
        }
        None if state.everyone.contains(target) => "Everyone in this group can ask the Agent.",
        None => "Only you can ask the Agent in this group.",
    }
    .into()
}
