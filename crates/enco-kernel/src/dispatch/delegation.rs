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
    /// The child a `session_send` call reaches, resolved when it settles, in call order, and
    /// what its settlement records and delivers.
    ///
    /// Without the child and without a profile there is nothing to address, and the call fails.
    pub(super) async fn delegation(
        &self,
        request: DelegationRequest,
    ) -> Result<Result<(Delegated, SessionDelivery), Failure>, StoreError> {
        let existing = self.deps.store.session_by_name(&request.name).await?;
        // The parent's Log is the authority over its children's names, so the creation record is
        // written here, with the settlement that delegates to the child.
        let (target, profile, working_dir) = match (existing, request.profile) {
            (Some(child), profile) => (
                DelegationTarget::Existing(SessionRef {
                    id: child.id,
                    name: child.name,
                }),
                profile,
                request.working_dir,
            ),
            (None, Some(profile)) => (
                DelegationTarget::Created(Creation {
                    id: SessionId::new(),
                    name: request.name,
                    parent: Some(SessionRef {
                        id: self.session.id,
                        name: self.session.name.clone(),
                    }),
                    created_at: self.deps.clock.now(),
                    node: self.session.binding.node,
                    profile,
                    working_dir: request
                        .working_dir
                        .unwrap_or_else(|| self.session.working_dir.clone()),
                }),
                None,
                None,
            ),
            (None, None) => {
                return Ok(Err(Failure {
                    code: code::TOOL_FAILED.into(),
                    message: format!(
                        "child Session `{}` does not exist; pass `profile` to create it",
                        request.name.short_name()
                    ),
                    retryable: false,
                }));
            }
        };
        let event = Event {
            id: EventId::new(),
            session: target.session().id,
            source: EventSource::Parent,
            body: EventBody::SessionMessage(Outgoing {
                text: request.text,
                file: None,
            }),
            received_at: self.deps.clock.now(),
        };
        Ok(Ok((
            Delegated {
                target,
                event: event.id,
            },
            SessionDelivery {
                event,
                kind: SessionDeliveryKind::Delegation {
                    profile,
                    working_dir,
                },
            },
        )))
    }
}
