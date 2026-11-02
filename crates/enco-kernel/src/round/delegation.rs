use super::*;
use std::collections::BTreeSet;

/// What a Session may do once its profile is narrowed by every ancestor's.
pub(super) struct Permissions {
    /// The tools it may be offered, by name.
    pub tools: BTreeSet<String>,
    /// Whether it may have the owner's instructions and memories.
    pub owner_context: bool,
}

impl SessionActor {
    /// This Session's profile and the permissions it keeps along its lineage.
    ///
    /// A Round with an ancestor whose profile cannot be resolved fails rather than widening them.
    pub(super) fn permissions<'a>(
        &self,
        settings: &'a KernelSettings,
        snapshot: &Snapshot,
        ancestors: &[SessionRecord],
    ) -> Result<(&'a Profile, Permissions), RoundError> {
        let profile = settings
            .profiles
            .get(&self.session.profile)
            .ok_or_else(|| unknown(&self.session))?;
        let mut tools: BTreeSet<_> = snapshot
            .tools
            .iter()
            .map(|tool| tool.id.name.clone())
            .collect();
        let mut owner_context = true;
        let policies = std::iter::once(Ok(profile)).chain(ancestors.iter().map(|ancestor| {
            settings
                .profiles
                .get(&ancestor.profile)
                .ok_or_else(|| unknown(ancestor))
        }));
        for policy in policies {
            let policy = policy?;
            owner_context &= policy.owner_context;
            if let Some(allowed) = &policy.tools {
                tools.retain(|name| allowed.contains(name));
            }
        }
        if ancestors.len() as u64 >= u64::from(settings.max_depth) {
            for name in crate::builtin::DELEGATION {
                tools.remove(name);
            }
        }
        Ok((
            profile,
            Permissions {
                tools,
                owner_context,
            },
        ))
    }

    pub(super) async fn children(&self) -> Result<Vec<Child>, StoreError> {
        let mut children = Vec::new();
        for child in self.deps.store.children(self.session.id).await? {
            children.push(Child {
                state: self.deps.store.child_state(child.id).await?,
                name: child.name.short_name().into(),
                profile: child.profile,
            });
        }
        Ok(children)
    }
}

pub(super) fn profiles(settings: &KernelSettings) -> Vec<DelegableProfile> {
    settings
        .profiles
        .iter()
        .map(|(name, profile)| DelegableProfile {
            name: name.clone(),
            description: profile.description.clone(),
            reply: profile
                .reply
                .iter()
                .map(|endpoint| ProfileEndpoint {
                    endpoint: endpoint.endpoint.name.clone(),
                    model: endpoint.endpoint.settings.model.clone(),
                    accepts: endpoint.accepts.clone(),
                    budget: endpoint.budget,
                })
                .collect(),
        })
        .collect()
}

fn unknown(session: &SessionRecord) -> RoundError {
    failed(
        code::PROFILE_UNKNOWN,
        format!(
            "session {} uses unknown profile {}",
            session.name, session.profile
        ),
    )
}
