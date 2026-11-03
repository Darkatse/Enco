//! Where a conversation goes, by the one rule messages and commands share, and the two commands
//! that change the conversation: `/clear` and `/compact`.
use super::*;

const NO_SESSION: &str = "This conversation has no Session yet; its first message starts one.";
const NOTHING_TO_COMPACT: &str = "Nothing to compact yet.";
const COMPACTION_REQUESTED: &str = "The conversation will be compacted when the next Run starts.";
const CLEARED: &str = "Cleared the conversation; nothing was deleted. To return, use /backtrack on \
    the last reply before this.";

/// Where a conversation goes now, and the archived Session its route led to, if it did.
pub(super) struct Routed {
    pub destination: Destination,
    pub archived: Option<SessionName>,
}

/// The Session a conversation goes to.
pub(super) enum Destination {
    /// Its live route.
    Routed(SessionId),
    /// `main`, which a message creates when it does not exist yet.
    Main,
    /// A root or group Session of its own, which only a message creates.
    New {
        profile: ProfileName,
        /// The conversation's own name, which names a root Session.
        name: Option<String>,
    },
}

impl Connection {
    /// Where `target` goes now: its live route, otherwise `main` or a Session of its own.
    ///
    /// `Err` is the refusal for a group chat the connection has no profile for.
    pub(super) async fn destination(
        &self,
        state: &State,
        target: &str,
        kind: &transport::ConversationKind,
    ) -> Result<Result<Routed, String>, ChannelError> {
        let (profile, name) = match kind {
            transport::ConversationKind::Main => (None, None),
            transport::ConversationKind::Root { name } => {
                (Some(ProfileName::default()), name.clone())
            }
            transport::ConversationKind::Group => match self.settings.group_profile.clone() {
                Some(profile) => (Some(profile), None),
                None => return Ok(Err(input::NO_GROUP_PROFILE.into())),
            },
        };
        let route = Route::of(&self.kernel, &state.chats, target).await?;
        let destination = match (&route, profile) {
            (Route::Live(session), _) => Destination::Routed(*session),
            (_, None) => Destination::Main,
            (_, Some(profile)) => Destination::New { profile, name },
        };
        let archived = match route {
            Route::Archived(name) => Some(name),
            Route::Unrouted | Route::Live(_) => None,
        };
        Ok(Ok(Routed {
            destination,
            archived,
        }))
    }

    /// What the owner learns when a conversation whose Session was archived goes to `now`.
    pub(super) async fn rerouted(
        &self,
        archived: Option<SessionName>,
        now: SessionId,
    ) -> Result<Option<String>, ChannelError> {
        let Some(archived) = archived else {
            return Ok(None);
        };
        let now = self.kernel.session(now).await?.name;
        Ok(Some(format!(
            "Session {archived} is archived; this conversation now goes to Session {now}."
        )))
    }

    /// The Session a conversation command acts on, routed as a message would route it.
    ///
    /// It comes with the notice for an archived route. `Err` is the receipt when there is none,
    /// since a command never creates a Session.
    pub(super) async fn addressed(
        &self,
        state: &mut State,
        target: &str,
        kind: &transport::ConversationKind,
    ) -> Result<Result<(SessionId, Option<String>), String>, ChannelError> {
        let Routed {
            destination,
            archived,
        } = match self.destination(state, target, kind).await? {
            Ok(routed) => routed,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let session = match destination {
            Destination::Routed(session) => Some(session),
            Destination::Main => match self.kernel.find_session("main").await {
                Ok(main) => Some(main.id),
                Err(KernelError::Lookup(_)) => None,
                Err(error) => return Err(error.into()),
            },
            Destination::New { .. } => None,
        };
        Ok(match session {
            Some(session) => {
                state.chats.insert(target.into(), session);
                Ok((session, self.rerouted(archived, session).await?))
            }
            None => Err(match archived {
                Some(archived) => format!(
                    "Session {archived} is archived; the next message here starts a new Session."
                ),
                None => NO_SESSION.into(),
            }),
        })
    }

    pub(super) async fn clear(
        &mut self,
        mut state: State,
        key: String,
        source: EventSource,
        target: &str,
        kind: &transport::ConversationKind,
    ) -> Result<Option<String>, ChannelError> {
        let (session, notice) = match self.addressed(&mut state, target, kind).await? {
            Ok(addressed) => addressed,
            Err(receipt) => return self.receipt(state, key, receipt).await,
        };
        let body = EventBody::Backtrack { to: None };
        let refusal = self.submit(state, session, source, body, key, None).await?;
        Ok(Some(
            refusal.unwrap_or_else(|| with_notice(notice, CLEARED)),
        ))
    }

    pub(super) async fn compact(
        &mut self,
        mut state: State,
        key: String,
        source: EventSource,
        target: &str,
        kind: &transport::ConversationKind,
    ) -> Result<Option<String>, ChannelError> {
        let (session, notice) = match self.addressed(&mut state, target, kind).await? {
            Ok(addressed) => addressed,
            Err(receipt) => return self.receipt(state, key, receipt).await,
        };
        if !self.kernel.can_request_compaction(session).await? {
            let receipt = with_notice(notice, NOTHING_TO_COMPACT);
            return self.receipt(state, key, receipt).await;
        }
        let refusal = self
            .submit(state, session, source, EventBody::Compact, key, None)
            .await?;
        Ok(Some(refusal.unwrap_or_else(|| {
            with_notice(notice, COMPACTION_REQUESTED)
        })))
    }
}

/// A command's receipt after the notice that its conversation was rerouted, if it was.
pub(super) fn with_notice(notice: Option<String>, receipt: &str) -> String {
    match notice {
        Some(notice) => format!("{notice}\n{receipt}"),
        None => receipt.into(),
    }
}
