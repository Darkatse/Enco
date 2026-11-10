use super::*;

pub(super) struct DelegationRequest {
    name: SessionName,
    text: String,
    profile: Option<ProfileName>,
    working_dir: Option<WorkingDir>,
}

impl DelegationRequest {
    pub fn parse(
        args: serde_json::Map<String, serde_json::Value>,
        parent: &SessionName,
        directory: &WorkingDir,
    ) -> Result<Self, Failure> {
        let args: crate::builtin::SendArguments = parse_arguments(args)?;
        let invalid = |error: String| Failure {
            code: code::TOOL_INVALID_ARGUMENTS.into(),
            message: error,
            retryable: false,
        };
        let name = parent
            .child(&args.to)
            .map_err(|error| invalid(error.to_string()))?;
        if args.text.trim().is_empty() {
            return Err(invalid(
                "text must include the task or follow-up message".into(),
            ));
        }
        let profile = args
            .profile
            .map(|name| {
                name.parse::<ProfileName>()
                    .map_err(|error| invalid(error.to_string()))
            })
            .transpose()?;
        let working_dir = args
            .working_dir
            .map(|path| {
                WorkingDir::new(directory.node, directory.path().join(path))
                    .map_err(|error| invalid(error.to_string()))
            })
            .transpose()?;
        Ok(Self {
            name,
            text: args.text,
            profile,
            working_dir,
        })
    }
}

impl SessionActor {
    pub(super) async fn delegation(
        &self,
        request: DelegationRequest,
    ) -> Result<(Delegated, SessionDelivery), StoreError> {
        let existing = self.deps.store.session_by_name(&request.name).await?;
        // The parent's Log is the authority over its children's names, so the creation record is
        // written here, with the settlement that delegates to the child. Without a profile there
        // is nothing to create, and the commit rejects the call.
        let existing = existing.map(|session| session.id);
        let creation = match (existing, &request.profile) {
            (Some(_), _) | (None, None) => None,
            (None, Some(profile)) => Some(Creation {
                id: SessionId::new(),
                name: request.name.clone(),
                parent: Some(SessionRef {
                    id: self.session.id,
                    name: self.session.name.clone(),
                }),
                created_at: self.deps.clock.now(),
                node: self.session.binding.node,
                profile: profile.clone(),
                working_dir: request
                    .working_dir
                    .clone()
                    .unwrap_or_else(|| self.session.working_dir.clone()),
            }),
        };
        let id = existing
            .or(creation.as_ref().map(|creation| creation.id))
            .unwrap_or_default();
        let event = Event {
            id: EventId::new(),
            session: id,
            source: EventSource::Parent,
            body: EventBody::SessionMessage(Outgoing {
                text: request.text,
                file: None,
            }),
            received_at: self.deps.clock.now(),
        };
        let name = request.name.clone();
        let delegated = Delegated {
            session: SessionRef {
                id,
                name: request.name,
            },
            creation: creation.clone(),
            event: event.id,
        };
        Ok((
            delegated,
            SessionDelivery {
                event,
                kind: SessionDeliveryKind::Delegation {
                    name,
                    create: creation.map(Box::new),
                    profile: request.profile,
                    working_dir: request.working_dir,
                },
            },
        ))
    }
}
