use super::*;

impl SessionActor {
    pub(super) async fn compose_input(
        &self,
        snapshot: &Snapshot,
        timezone: Tz,
        safe_mode: Option<SafeMode>,
        reply: ReplyTarget,
        compaction: Option<Budget>,
        token: &CancellationToken,
    ) -> Result<ComposeInput, RoundError> {
        let now = self.deps.clock.now();
        let latest_event = self.entries.iter().rev().find_map(|e| match &e.body {
            EntryBody::EventConsumed { event } => Some(event.clone()),
            _ => None,
        });
        let new_input = self
            .entries
            .iter()
            .rev()
            .take_while(|entry| !matches!(entry.body, EntryBody::RoundEnded { .. }))
            .any(|entry| matches!(entry.body, EntryBody::EventConsumed { .. }));
        let query = ContextQuery {
            session: self.session.clone(),
            latest_event,
            new_input,
            cancel: token.child_token(),
        };
        let mut context = Contribution::default();
        if safe_mode.is_none() {
            for source in &snapshot.context {
                cancelled(token)?;
                let contribution = source.contribute(&query).await;
                cancelled(token)?;
                let mut contribution =
                    contribution.map_err(|e| failed(code::CONTEXT_FAILED, e.to_string()))?;
                context.candidates.append(&mut contribution.candidates);
                context.omitted.append(&mut contribution.omitted);
            }
        }
        let tools = snapshot
            .tools
            .iter()
            .filter(|t| safe_mode.is_none() || snapshot.lifeline.contains(&t.id))
            .map(|t| (t.id.clone(), t.spec.clone()))
            .collect();
        let previous_plan = self.previous_plan(&mut context.omitted).await?;
        Ok(ComposeInput {
            now,
            timezone,
            session: self.session.clone(),
            transcript: crate::transcript::project(&self.entries),
            previous_plan,
            context,
            tools,
            safe_mode,
            reply,
            compaction,
            compactions_left: limits::MAX_COMPACTIONS_PER_ROUND,
        })
    }

    async fn previous_plan(
        &self,
        omitted: &mut Vec<Omission>,
    ) -> Result<Option<ContextPlan>, RoundError> {
        let Some(hash) = self
            .entries
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
        match self.deps.store.get_blob(hash).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
                failed(code::PLAN_INVALID, format!("previous plan {hash}: {error}"))
            }),
            Err(StoreError::Blob(_)) => {
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
