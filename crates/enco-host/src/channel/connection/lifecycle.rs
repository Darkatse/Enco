//! Owner commands accept protocol progress before changing a Session's execution lifecycle.
use super::*;

/// Group chats address only their own Session, as with `/session`.
const GROUP_LIFECYCLE: &str = "In a group chat, /halt stops only this group's Session; halt other Sessions by name, or archive Sessions, in a conversation private to you.";

impl Connection {
    pub(super) async fn cancel_session(
        &mut self,
        mut state: State,
        key: String,
        (target, kind): (&str, &transport::ConversationKind),
    ) -> Result<Option<String>, ChannelError> {
        let (session, notice) = match self.addressed(&mut state, target, kind).await? {
            Ok(addressed) => addressed,
            Err(receipt) => return self.receipt(state, key, receipt).await,
        };
        self.commit_update(state, key, None, None).await?;
        let receipt = match self.kernel.cancel(session) {
            Ok(true) => "Cancellation requested.".into(),
            Ok(false) => "No active Run.".into(),
            Err(error) => format!("Cancel failed: {error}"),
        };
        Ok(Some(conversation::with_notice(notice, &receipt)))
    }

    pub(super) async fn stop_session(
        &mut self,
        mut state: State,
        key: String,
        source: EventSource,
        (target, kind): (&str, &transport::ConversationKind),
        name: Option<String>,
        archive: bool,
    ) -> Result<Option<String>, ChannelError> {
        if !kind.private() && (archive || name.is_some()) {
            return self.receipt(state, key, GROUP_LIFECYCLE.into()).await;
        }
        if archive && name.is_none() {
            let listing = self.sessions(target).await?;
            return self.receipt(state, key, listing).await;
        }
        let (session, notice) = match name.as_deref() {
            Some(name) => match self.kernel.find_session(name).await {
                Ok(session) if session.archived_at.is_none() => (session, None),
                Ok(session) => {
                    let receipt = format!("Session {} is archived.", session.name);
                    return self.receipt(state, key, receipt).await;
                }
                Err(KernelError::Lookup(error)) => {
                    return self.receipt(state, key, error.to_string()).await;
                }
                Err(error) => return Err(error.into()),
            },
            None => match self.addressed(&mut state, target, kind).await? {
                Ok((session, notice)) => (self.kernel.session(session).await?, notice),
                Err(receipt) => return self.receipt(state, key, receipt).await,
            },
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
        Ok(Some(conversation::with_notice(notice, &receipt)))
    }
}
