//! Owner commands accept protocol progress before changing a Session's execution lifecycle.
use super::*;

/// Group chats address only their own Session, as with `/session`.
const GROUP_LIFECYCLE: &str = "In a group chat, /halt stops only this group's Session; halt other Sessions by name, or archive Sessions, in a conversation private to you.";

impl Connection {
    pub(super) async fn cancel_session(
        &mut self,
        state: State,
        key: String,
        target: &str,
    ) -> Result<Option<String>, ChannelError> {
        self.commit_update(state, key, None, None).await?;
        let result = self
            .state
            .chats
            .get(target)
            .map(|session| self.kernel.cancel(*session))
            .transpose();
        Ok(Some(match result {
            Ok(Some(true)) => "Cancellation requested.".into(),
            Ok(_) => "No active Run.".into(),
            Err(error) => format!("Cancel failed: {error}"),
        }))
    }

    pub(super) async fn stop_session(
        &mut self,
        state: State,
        key: String,
        source: EventSource,
        (target, private): (&str, bool),
        name: Option<String>,
        archive: bool,
    ) -> Result<Option<String>, ChannelError> {
        if !private && (archive || name.is_some()) {
            return self.receipt(state, key, GROUP_LIFECYCLE.into()).await;
        }
        if archive && name.is_none() {
            let listing = self.sessions(target).await?;
            return self.receipt(state, key, listing).await;
        }
        let found = match name.as_deref() {
            Some(name) => self.kernel.find_session(name).await,
            None => match state.chats.get(target) {
                Some(session) => self.kernel.session(*session).await,
                None => return self.receipt(state, key, "No active Session.".into()).await,
            },
        };
        let session = match found {
            Ok(session) if session.archived_at.is_none() => session,
            Ok(_) if name.is_none() => {
                return self.receipt(state, key, "No active Session.".into()).await;
            }
            Ok(session) => {
                return self
                    .receipt(state, key, format!("Session {} is archived.", session.name))
                    .await;
            }
            Err(KernelError::Lookup(error)) => {
                return self.receipt(state, key, error.to_string()).await;
            }
            Err(error) => return Err(error.into()),
        };
        self.commit_update(state, key, None, None).await?;
        let (verb, halted) = if archive {
            ("Archived", self.kernel.archive(session.id, source).await?)
        } else {
            ("Halted", self.kernel.halt(session.id, source).await?)
        };
        let mut receipt = format!("{verb} {} and its children.", session.name);
        for stopped in &halted.stopped {
            receipt.push_str(&format!(
                " {} had already stopped after a failure and takes the halt once it runs again; enco status shows why.",
                stopped.name
            ));
        }
        Ok(Some(receipt))
    }
}
