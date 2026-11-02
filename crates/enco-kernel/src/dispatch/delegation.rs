use super::*;

pub(super) enum Execution {
    Result(Outcome),
    Delegation(DelegationRequest),
}

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
        let created = existing.is_none();
        let (id, create) = if let Some(session) = existing {
            (session.id, None)
        } else if let Some(profile) = &request.profile {
            let id = SessionId::new();
            (
                id,
                Some(Box::new(NewSession {
                    id,
                    name: request.name.clone(),
                    profile: profile.clone(),
                    origin: NewSessionOrigin::Child {
                        parent: self.session.id,
                        working_dir: request
                            .working_dir
                            .clone()
                            .unwrap_or_else(|| self.session.working_dir.clone()),
                    },
                    created_at: self.deps.clock.now(),
                })),
            )
        } else {
            (SessionId::new(), None)
        };
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
            created,
            event: event.id,
        };
        Ok((
            delegated,
            SessionDelivery {
                event,
                kind: SessionDeliveryKind::Delegation {
                    name,
                    create,
                    profile: request.profile,
                    working_dir: request.working_dir,
                },
            },
        ))
    }
}
