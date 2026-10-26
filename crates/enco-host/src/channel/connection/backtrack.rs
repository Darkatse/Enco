//! The owner's backtrack command, submitted only when the message it replies to is in the
//! conversation's current Session.
//!
//! The message is found among the facts that recorded it: the owner's inputs in the Log and this
//! connection's delivery settlements.
use super::*;

const TO_AGENT_MESSAGE: &str = "Back at that message. Later messages are no longer part of the \
    conversation; changes they made outside it were not undone.";
const TO_OWNER_MESSAGE: &str = "Back at your message; replying again.";
const NO_REPLY: &str = "Send this as a reply to the message to go back to.";
const NOT_RECORDED: &str = "That message is not part of any conversation.";

/// A message the owner pointed at, where it was recorded and how to acknowledge going back to it.
struct Target {
    session: SessionId,
    pos: LogPos,
    receipt: &'static str,
}

impl Connection {
    /// Submit a backtrack to the message `reply` names, or refuse it; the protocol state advances
    /// either way, and the returned receipt tells the owner which happened.
    pub(super) async fn backtrack(
        &mut self,
        state: State,
        conversation: &str,
        message: String,
        reply: Option<String>,
    ) -> Result<String, ChannelError> {
        match self.target(&state, conversation, reply.as_deref()).await? {
            Ok(Target {
                session,
                pos,
                receipt,
            }) => {
                let body = EventBody::Backtrack { to: pos };
                self.submit(state, session, conversation, message, body)
                    .await?;
                Ok(receipt.into())
            }
            Err(refusal) => {
                self.commit(state, &[], None).await?;
                Ok(refusal)
            }
        }
    }

    /// Where the replied-to message was recorded, or why the conversation cannot go back to it.
    async fn target(
        &self,
        state: &State,
        conversation: &str,
        reply: Option<&str>,
    ) -> Result<Result<Target, String>, ChannelError> {
        let Some(reply) = reply else {
            return Ok(Err(NO_REPLY.into()));
        };
        let Identity { channel, account } = &self.identity;
        let consumed = self
            .kernel
            .consumed_message(channel, account, conversation, reply)
            .await?;
        let target = match consumed {
            Some((session, pos)) => Target {
                session,
                pos,
                receipt: TO_OWNER_MESSAGE,
            },
            None => {
                let delivered = self
                    .kernel
                    .delivered_message(&self.identity.key(), conversation, reply)
                    .await?;
                let Some(Delivery { session, pos, .. }) = delivered else {
                    return Ok(Err(NOT_RECORDED.into()));
                };
                Target {
                    session,
                    pos,
                    receipt: TO_AGENT_MESSAGE,
                }
            }
        };
        if state.chats.get(conversation) != Some(&target.session) {
            let name = self.session_name(target.session).await?;
            return Ok(Err(format!(
                "That message is in Session {name}; switch to it first."
            )));
        }
        Ok(Ok(target))
    }
}
