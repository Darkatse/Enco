use super::{selection::Combination, *};
use std::{collections::HashMap, path::PathBuf};

/// What the service reported for the previous plan, and the combination whose tokenizer counted
/// it; another model counts the same request differently.
pub(super) struct Measured {
    pub usage: Usage,
    pub combination: Combination,
}

/// Facts gathered once per Round; a later composition changes only its targets and Transcript.
pub(super) struct Gathered {
    pub owner_instructions: Option<String>,
    pub profile_instructions: Option<String>,
    pub memories: Vec<Memory>,
    pub omitted: Vec<Omission>,
    pub previous_plan: Option<ContextPlan>,
    pub measured: Option<Measured>,
    pub compaction_requested: bool,
    pub transcript: Transcript,
    pub blob_paths: HashMap<ContentHash, PathBuf>,
}

impl SessionActor {
    pub(super) async fn gather(
        &self,
        snapshot: &Snapshot,
        safe_mode: bool,
        owner_context: bool,
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
        let profile_instructions = if safe_mode {
            None
        } else {
            contribute(snapshot.profile_instructions.as_ref(), &query, token).await?
        };
        let (owner_instructions, offered) = if safe_mode || !owner_context {
            (None, OfferedMemories::default())
        } else {
            (
                contribute(snapshot.owner_instructions.as_ref(), &query, token).await?,
                contribute(snapshot.memories.as_ref(), &query, token).await?,
            )
        };
        let mut omitted = offered.omitted;
        let (previous_plan, measured) = self.previous_plan(&conversation, &mut omitted).await?;
        let (transcript, blob_paths, compaction_requested) = self.transcript();
        Ok(Gathered {
            owner_instructions,
            profile_instructions,
            memories: offered.memories,
            omitted,
            previous_plan,
            measured,
            compaction_requested,
            transcript,
            blob_paths,
        })
    }

    /// The conversation's transcript, where each blob it refers to is stored, and whether it
    /// carries the owner's compaction request; a compaction changes all three.
    pub(super) fn transcript(&self) -> (Transcript, HashMap<ContentHash, PathBuf>, bool) {
        let conversation = crate::transcript::conversation(&self.entries);
        let transcript = crate::transcript::project(&conversation);
        let blob_paths = transcript
            .items
            .iter()
            .flat_map(|item| {
                let images = item.message.parts.iter().filter_map(|part| match part {
                    Part::Image(image) => Some(image.content),
                    _ => None,
                });
                images.chain(item.event.iter().flat_map(Event::blobs))
            })
            .map(|content| (content, self.deps.store.blob_path(&content)))
            .collect();
        let requested = crate::transcript::compaction_requested(&conversation, &[]);
        (transcript, blob_paths, requested)
    }

    async fn previous_plan(
        &self,
        conversation: &[&Entry],
        omitted: &mut Vec<Omission>,
    ) -> Result<(Option<ContextPlan>, Option<Measured>), RoundError> {
        let Some(last) = crate::transcript::last_reply(conversation) else {
            return Ok((None, None));
        };
        match self.deps.store.plan(last.plan).await {
            Ok(plan) => Ok((
                Some(plan),
                Some(Measured {
                    usage: last.usage.clone(),
                    combination: Combination {
                        settings: last.settings.clone(),
                        provider: last.provider,
                    },
                }),
            )),
            Err(StoreError::Plan { .. }) => {
                omitted.push(Omission {
                    source: format!("previous-plan:{}", last.plan),
                    reason: "previous plan is missing or corrupt".into(),
                });
                Ok((None, None))
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
