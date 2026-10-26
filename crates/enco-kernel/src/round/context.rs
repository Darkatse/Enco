use super::*;

/// Facts gathered once per Round; a later composition changes only its targets and Transcript.
pub(super) struct Gathered {
    pub instructions: Option<String>,
    pub memories: Vec<Memory>,
    pub omitted: Vec<Omission>,
    pub previous_plan: Option<ContextPlan>,
    pub transcript: Transcript<LocatedImage>,
}

impl SessionActor {
    pub(super) async fn gather(
        &self,
        snapshot: &Snapshot,
        safe_mode: bool,
        token: &CancellationToken,
    ) -> Result<Gathered, RoundError> {
        // A Round reads its history from the conversation as it now stands, not the whole Log.
        let conversation = crate::transcript::conversation(&self.entries);
        let latest_event = conversation
            .iter()
            .rev()
            .find_map(|entry| crate::transcript::input(entry).cloned());
        let new_input = conversation
            .iter()
            .rev()
            .take_while(|entry| !matches!(entry.body, EntryBody::RoundEnded { .. }))
            .any(|entry| crate::transcript::input(entry).is_some());
        let query = ContextQuery {
            session: self.session.clone(),
            latest_event,
            new_input,
            cancel: token.child_token(),
        };
        let (instructions, offered) = if safe_mode {
            (None, OfferedMemories::default())
        } else {
            (
                contribute(snapshot.instructions.as_ref(), &query, token).await?,
                contribute(snapshot.memories.as_ref(), &query, token).await?,
            )
        };
        let mut omitted = offered.omitted;
        let previous_plan = self.previous_plan(&conversation, &mut omitted).await?;
        Ok(Gathered {
            instructions,
            memories: offered.memories,
            omitted,
            previous_plan,
            transcript: self.located_transcript(),
        })
    }

    pub(super) fn located_transcript(&self) -> Transcript<LocatedImage> {
        let conversation = crate::transcript::conversation(&self.entries);
        crate::transcript::project(&conversation).map_images(|image| LocatedImage {
            path: self.deps.store.blob_path(&image.content),
            image,
        })
    }

    async fn previous_plan(
        &self,
        conversation: &[&Entry],
        omitted: &mut Vec<Omission>,
    ) -> Result<Option<ContextPlan>, RoundError> {
        let Some(hash) = conversation
            .iter()
            .rev()
            .find_map(|entry| match &entry.body {
                EntryBody::AttemptStarted {
                    purpose: AttemptPurpose::Reply,
                    plan,
                    ..
                } => Some(plan),
                _ => None,
            })
        else {
            return Ok(None);
        };
        match self.deps.store.plan(hash).await {
            Ok(plan) => Ok(Some(plan)),
            Err(StoreError::Plan { .. }) => {
                omitted.push(Omission {
                    source: format!("previous-plan:{hash}"),
                    reason: "previous plan is missing or corrupt".into(),
                });
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// Cancellation is checked before and after each source, so a cancelled Round asks no further.
async fn contribute<T: Send>(
    source: &dyn ContextSource<Supply = T>,
    query: &ContextQuery,
    token: &CancellationToken,
) -> Result<T, RoundError> {
    cancelled(token)?;
    let supplied = source.contribute(query).await;
    cancelled(token)?;
    supplied.map_err(|e| failed(code::CONTEXT_FAILED, e.to_string()))
}
