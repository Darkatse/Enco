use crate::{ComposeInput, Exports, ProviderRequest, Transcript};
use enco_core::*;
use std::collections::HashSet;

#[derive(Debug, thiserror::Error)]
#[error("invalid context plan: {0}")]
pub(crate) struct PlanError(pub String);

pub(crate) fn validate(
    plan: &ContextPlan,
    input: &ComposeInput,
    purpose: AttemptPurpose,
    lifeline: &[CapabilityId],
    requires_lifeline: bool,
) -> Result<(), PlanError> {
    if plan.items.is_empty() {
        return Err(PlanError("no messages".into()));
    }
    for item in &plan.items {
        if let PlanItem::Message { message, .. } = item
            && message
                .parts
                .iter()
                .any(|part| !matches!(part, Part::Text { .. }))
        {
            return Err(PlanError("inline messages must contain only text".into()));
        }
    }
    let messages = messages(plan, &input.transcript)?;
    let images: Vec<_> = messages
        .iter()
        .flat_map(|(message, _)| &message.parts)
        .filter_map(|part| match part {
            Part::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    if !images.is_empty()
        && (purpose == AttemptPurpose::Compaction
            || !input.reply.accepts.contains(&Modality::Image))
    {
        return Err(PlanError(
            "this request target does not accept images".into(),
        ));
    }
    validate_messages(&messages)?;
    let mut names = HashSet::new();
    for (id, spec) in &plan.tools {
        if !names.insert(&spec.name)
            || !input
                .tools
                .iter()
                .any(|(available_id, available_spec)| available_id == id && available_spec == spec)
        {
            return Err(PlanError(format!(
                "tool {id} is duplicated or differs from the Round snapshot"
            )));
        }
    }
    if purpose == AttemptPurpose::Compaction && !plan.tools.is_empty() {
        return Err(PlanError("compaction must not disclose tools".into()));
    }
    if purpose == AttemptPurpose::Reply
        && requires_lifeline
        && lifeline
            .iter()
            .any(|id| !plan.tools.iter().any(|(given, _)| given == id))
    {
        return Err(PlanError("reply omitted a required lifeline tool".into()));
    }
    Ok(())
}

pub(crate) fn resolve(
    plan: &ContextPlan,
    transcript: &Transcript,
    exports: &Exports,
    target: PluginId,
) -> Result<ProviderRequest<Image>, PlanError> {
    Ok(ProviderRequest {
        messages: messages(plan, transcript)?
            .into_iter()
            .map(|(message, generation)| {
                let own_extensions =
                    generation.and_then(|id| exports.plugin_of(id)) == Some(target);
                Message {
                    role: message.role,
                    parts: message
                        .parts
                        .into_iter()
                        .filter(|part| own_extensions || !matches!(part, Part::Extension(_)))
                        .collect(),
                }
            })
            .collect(),
        tools: plan.tools.iter().map(|(_, spec)| spec.clone()).collect(),
        max_output_tokens: plan.max_output_tokens,
    })
}

/// An entry with nothing left once its images stay behind, such as a photo alone, sends no
/// message; the composer's note before it describes the entry.
fn messages(
    plan: &ContextPlan,
    transcript: &Transcript,
) -> Result<Vec<(Message, Option<GenerationId>)>, PlanError> {
    let mut messages = Vec::with_capacity(plan.items.len());
    for item in &plan.items {
        match item {
            PlanItem::Message { message, .. } => messages.push((message.clone(), None)),
            PlanItem::Log { pos, images } => {
                let item = transcript
                    .items
                    .binary_search_by_key(pos, |item| item.pos)
                    .map(|index| &transcript.items[index])
                    .map_err(|_| {
                        PlanError(format!(
                            "Log position {pos:?} is not in the recorded transcript"
                        ))
                    })?;
                let mut message = item.message.clone();
                if !images {
                    message.parts.retain(|part| !matches!(part, Part::Image(_)));
                }
                if !message.parts.is_empty() {
                    messages.push((message, item.generation));
                }
            }
        }
    }
    Ok(messages)
}

fn validate_messages(messages: &[(Message, Option<GenerationId>)]) -> Result<(), PlanError> {
    let mut pending = std::collections::HashMap::new();
    for (message, _) in messages {
        if message.role == Role::Tool {
            let Some((Part::ToolResult(result), images)) = message.parts.split_first() else {
                return Err(PlanError("tool message must start with its result".into()));
            };
            if images.iter().any(|part| !matches!(part, Part::Image(_))) {
                return Err(PlanError(
                    "tool result may only be followed by images".into(),
                ));
            }
            if pending.remove(&result.call) != Some(result.provider_id.as_str()) {
                return Err(PlanError("tool result has no matching pending call".into()));
            }
            continue;
        }
        if !pending.is_empty() {
            return Err(PlanError(
                "assistant calls are missing adjacent tool results".into(),
            ));
        }
        for part in &message.parts {
            match (&message.role, part) {
                (Role::System | Role::User | Role::Assistant, Part::Text { .. })
                | (Role::Assistant, Part::Extension(_))
                | (Role::User, Part::Image(_)) => {}
                (Role::Assistant, Part::ToolCall(call)) => {
                    if pending.insert(call.id, call.provider_id.as_str()).is_some() {
                        return Err(PlanError("duplicate call identity".into()));
                    }
                }
                _ => {
                    return Err(PlanError(
                        "message contains content incompatible with its role".into(),
                    ));
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(PlanError(
            "request ends before tool calls are settled".into(),
        ));
    }
    Ok(())
}
