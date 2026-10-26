use crate::{ComposeInput, CompositionItem, CompositionPlan, Exports, ProviderRequest, Transcript};
use enco_core::*;
use std::{borrow::Borrow, collections::HashSet};

#[derive(Debug, thiserror::Error)]
#[error("invalid context plan: {0}")]
pub(crate) struct PlanError(pub String);

pub(crate) fn bind(
    draft: CompositionPlan,
    input: &ComposeInput,
    purpose: AttemptPurpose,
) -> Result<ContextPlan, PlanError> {
    let mut names = HashSet::new();
    let tools = draft
        .tools
        .into_iter()
        .map(|id| {
            if !names.insert(id.name.clone()) {
                return Err(PlanError(format!("tool {id} is duplicated")));
            }
            let (_, spec) = input
                .tools
                .iter()
                .find(|(available, _)| *available == id)
                .ok_or_else(|| PlanError(format!("tool {id} is outside the Round snapshot")))?;
            Ok((id, DisclosedTool::from(spec)))
        })
        .collect::<Result<_, _>>()?;
    let plan = ContextPlan {
        items: draft
            .items
            .into_iter()
            .map(|item| match item {
                CompositionItem::Message {
                    role,
                    text,
                    sources,
                } => PlanItem::Message {
                    message: Message::text(role, text),
                    sources,
                },
                CompositionItem::Log { pos, images } => PlanItem::Log { pos, images },
            })
            .collect(),
        tools,
        max_output_tokens: draft.max_output_tokens,
        omitted: draft
            .omitted
            .into_iter()
            .chain(input.omitted.iter().cloned())
            .collect(),
    };
    if plan.items.is_empty() {
        return Err(PlanError("no messages".into()));
    }
    let messages = messages(&plan, &input.transcript)?;
    let has_images = messages
        .iter()
        .flat_map(|(message, _)| &message.parts)
        .any(|part| matches!(part, Part::Image(_)));
    if has_images
        && (purpose == AttemptPurpose::Compaction
            || !input.reply.accepts.contains(&Modality::Image))
    {
        return Err(PlanError(
            "this request target does not accept images".into(),
        ));
    }
    validate_messages(&messages)?;
    if purpose == AttemptPurpose::Compaction && !plan.tools.is_empty() {
        return Err(PlanError("compaction must not disclose tools".into()));
    }
    if purpose == AttemptPurpose::Reply
        && input
            .lifeline
            .iter()
            .any(|id| !plan.tools.iter().any(|(given, _)| given == id))
    {
        return Err(PlanError("reply omitted a required lifeline tool".into()));
    }
    Ok(plan)
}

pub(crate) fn resolve<I: Borrow<Image> + Clone>(
    plan: &ContextPlan,
    transcript: &Transcript<I>,
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
fn messages<I: Borrow<Image> + Clone>(
    plan: &ContextPlan,
    transcript: &Transcript<I>,
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
                let message = Message {
                    role: item.message.role,
                    parts: item
                        .message
                        .parts
                        .iter()
                        .cloned()
                        .filter_map(|part| match part.retype() {
                            Ok(part) => Some(part),
                            Err(image) => images.then(|| Part::Image(image.borrow().clone())),
                        })
                        .collect(),
                };
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
