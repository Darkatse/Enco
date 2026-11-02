use super::*;

/// Why a Session name or ID given by the owner did not resolve to a Session to act on.
#[derive(Debug, thiserror::Error)]
pub enum SessionLookupError {
    /// The name breaks the rule for name segments.
    #[error(transparent)]
    InvalidName(#[from] InvalidSessionName),
    /// No Session has this ID, and no unarchived Session has this name.
    #[error("unknown session {0}")]
    Unknown(String),
    /// Only archived Sessions answer to this name or ID; their history stays readable by ID.
    #[error(
        "session {name} is archived; address its history by ID: {}",
        .ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    Archived {
        /// The name the caller gave, or the archived Session's name when it gave an ID.
        name: String,
        /// The archived Sessions that had that name.
        ids: Vec<SessionId>,
    },
    /// A child Session that does not exist yet; only its parent creates it.
    #[error("child Session {0} does not exist; only its parent can create it")]
    ChildNotFound(SessionName),
}

/// A halted subtree, as far as its actors could take the halt.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Halted {
    /// The Sessions halted, root first; empty when the root was already archived.
    pub sessions: Vec<SessionRef>,
    /// Halted Sessions whose actor had stopped after a failure; each takes its halt once it runs
    /// again.
    pub stopped: Vec<SessionRef>,
}

impl Kernel {
    /// Read a recorded Session by its permanent identity, whether active or archived.
    pub async fn session(&self, id: SessionId) -> Result<SessionRecord, KernelError> {
        self.deps
            .store
            .session(id)
            .await?
            .ok_or(KernelError::UnknownSession(id))
    }

    /// Resolve an unarchived Session's name, or any recorded Session's ID, archived ones included.
    pub async fn find_session(&self, selector: &str) -> Result<SessionRecord, KernelError> {
        if let Ok(id) = selector.parse::<SessionId>() {
            return Ok(self
                .deps
                .store
                .session(id)
                .await?
                .ok_or_else(|| SessionLookupError::Unknown(selector.into()))?);
        }
        if let Some(session) = self.deps.store.session_by_name(selector).await? {
            return Ok(session);
        }
        let archived: Vec<_> = self
            .deps
            .store
            .sessions()
            .await?
            .into_iter()
            .filter(|session| session.name.as_str() == selector)
            .map(|session| session.id)
            .collect();
        Err(if archived.is_empty() {
            SessionLookupError::Unknown(selector.into())
        } else {
            SessionLookupError::Archived {
                name: selector.into(),
                ids: archived,
            }
        }
        .into())
    }

    /// Open an unarchived Session, creating only root names.
    pub async fn open_session(&self, selector: &str) -> Result<SessionRecord, KernelError> {
        let session = if selector.parse::<SessionId>().is_ok() {
            self.find_session(selector).await?
        } else {
            let name: SessionName = selector.parse().map_err(SessionLookupError::from)?;
            if name.is_root() {
                self.deps
                    .store
                    .ensure_session(&name, self.deps.clock.now())
                    .await?
            } else {
                match self.find_session(name.as_str()).await {
                    Err(KernelError::Lookup(SessionLookupError::Unknown(_))) => {
                        return Err(SessionLookupError::ChildNotFound(name).into());
                    }
                    found => found?,
                }
            }
        };
        if session.archived_at.is_some() {
            return Err(SessionLookupError::Archived {
                name: session.name.into(),
                ids: vec![session.id],
            }
            .into());
        }
        self.ensure_actor(session.clone())?;
        Ok(session)
    }

    /// Halt an unarchived subtree and wait until no halt waits in its running Sessions' Inboxes.
    pub async fn halt(
        &self,
        session: SessionId,
        source: EventSource,
    ) -> Result<Halted, KernelError> {
        self.deps.subtrees.stop(session, source, None, false).await
    }

    /// Stop accepting input for a subtree, halt it, and wait until its actors have stopped.
    pub async fn archive(
        &self,
        session: SessionId,
        source: EventSource,
    ) -> Result<Halted, KernelError> {
        self.deps.subtrees.stop(session, source, None, true).await
    }
}
