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
) -> Result<(), PlanError> {
    if plan.items.is_empty() {
        return Err(PlanError("no messages".into()));
    }
    validate_messages(&messages(plan, &input.transcript)?)?;
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
        && input.profile.requires_lifeline
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
) -> Result<ProviderRequest, PlanError> {
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
                        .iter()
                        .filter(|part| own_extensions || !matches!(part, Part::Extension(_)))
                        .cloned()
                        .collect(),
                }
            })
            .collect(),
        tools: plan.tools.iter().map(|(_, spec)| spec.clone()).collect(),
        max_output_tokens: plan.max_output_tokens,
    })
}

fn messages<'a>(
    plan: &'a ContextPlan,
    transcript: &'a Transcript,
) -> Result<Vec<(&'a Message, Option<GenerationId>)>, PlanError> {
    plan.items
        .iter()
        .map(|item| match item {
            PlanItem::Message { message } => Ok((message, None)),
            PlanItem::Log { pos } => transcript
                .items
                .binary_search_by_key(pos, |item| item.pos)
                .map(|index| {
                    let item = &transcript.items[index];
                    (&item.message, item.generation)
                })
                .map_err(|_| {
                    PlanError(format!(
                        "Log position {pos:?} is not in the recorded transcript"
                    ))
                }),
        })
        .collect()
}

fn validate_messages(messages: &[(&Message, Option<GenerationId>)]) -> Result<(), PlanError> {
    let mut pending = std::collections::HashMap::new();
    for (message, _) in messages {
        if message.role == Role::Tool {
            let [Part::ToolResult(result)] = message.parts.as_slice() else {
                return Err(PlanError(
                    "tool message must have exactly one result".into(),
                ));
            };
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
                | (Role::Assistant, Part::Extension(_)) => {}
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
