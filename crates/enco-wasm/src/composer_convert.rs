//! Converts the complete composer snapshot and accepts plans at the guest boundary.
//!
//! Records are destructured in full in both directions, so a field added on either side fails to
//! compile until someone decides how it crosses.
use crate::{
    bindings::{composers as wire, enco::plugin::types},
    composer_events as event, convert,
};
use enco_core::*;
use enco_kernel::{
    Budget, ComposeInput, Composition, CompositionItem, CompositionPlan, EndpointFailure,
    LocatedImage, ReplyTarget, SafeMode, Transcript, TranscriptItem,
};

pub(super) fn input(input: &ComposeInput) -> Result<wire::ComposeInput, Failure> {
    let ComposeInput {
        // The Round's identity and instant configure the composer's WASI clock and randomness.
        round: _,
        now: _,
        home,
        timezone,
        session,
        transcript,
        previous_plan,
        instructions,
        memories,
        omitted,
        unavailable,
        tools,
        lifeline,
        safe_mode,
        reply,
        compaction,
        compactions_left,
    } = input;
    Ok(wire::ComposeInput {
        session: session_record(session)?,
        timezone: timezone.to_string(),
        home: home.to_string_lossy().into_owned(),
        transcript: transcript_record(transcript)?,
        previous: previous_plan.as_ref().map(previous),
        instructions: instructions.clone(),
        memories: memories.iter().map(memory).collect::<Result<_, _>>()?,
        omitted: omitted.iter().map(omission).collect(),
        capabilities: tools
            .iter()
            .map(|(id, spec)| wire::Capability {
                id: capability(id),
                spec: tool_spec(spec),
            })
            .collect(),
        lifeline: lifeline.iter().map(capability).collect(),
        unavailable: unavailable
            .iter()
            .map(|(plugin, failure)| wire::UnavailablePlugin {
                plugin: plugin.clone(),
                failure: event::failure(failure),
            })
            .collect(),
        safe_mode: safe_mode.as_ref().map(|mode| match mode {
            SafeMode::Owner => wire::SafeMode::Owner,
            SafeMode::Failed(failure) => wire::SafeMode::Failed(event::failure(failure)),
        }),
        reply: reply_target(reply),
        compaction: compaction.map(budget),
        compactions_left: *compactions_left,
    })
}

fn session_record(session: &SessionRecord) -> Result<wire::Session, Failure> {
    // The Binding names the node that executes the Session, not a fact of the conversation.
    let SessionRecord {
        id,
        name,
        created_at,
        binding: _,
        profile,
    } = session;
    Ok(wire::Session {
        id: id.to_string(),
        name: name.clone(),
        profile: profile.clone(),
        created_at: event::datetime(*created_at)?,
    })
}

fn memory(memory: &Memory) -> Result<wire::Memory, Failure> {
    let Memory {
        id,
        text,
        pinned,
        created_at,
        updated_at,
        rev,
    } = memory;
    Ok(wire::Memory {
        id: id.to_string(),
        text: text.clone(),
        pinned: *pinned,
        created_at: event::datetime(*created_at)?,
        updated_at: event::datetime(*updated_at)?,
        rev: *rev,
    })
}

fn tool_spec(spec: &ToolSpec) -> types::ToolSpec {
    let ToolSpec {
        name,
        description,
        input_schema,
        effect,
    } = spec;
    types::ToolSpec {
        name: name.clone(),
        description: description.clone(),
        input_schema: input_schema.to_value().to_string(),
        effect: (*effect).into(),
    }
}

fn reply_target(reply: &ReplyTarget) -> wire::ReplyTarget {
    let ReplyTarget {
        budget: limits,
        accepts,
        skipped,
    } = reply;
    wire::ReplyTarget {
        budget: budget(*limits),
        accepts: accepts
            .iter()
            .map(|modality| match modality {
                Modality::Image => wire::Modality::Image,
            })
            .collect(),
        skipped: skipped
            .iter()
            .map(
                |EndpointFailure { endpoint, failure }| wire::SkippedEndpoint {
                    endpoint: endpoint.clone(),
                    failure: event::failure(failure),
                },
            )
            .collect(),
    }
}

fn transcript_record(transcript: &Transcript<LocatedImage>) -> Result<wire::Transcript, Failure> {
    let Transcript {
        summary,
        items,
        round_ends,
        run_ends,
    } = transcript;
    Ok(wire::Transcript {
        summary: summary.clone(),
        items: items
            .iter()
            .map(transcript_item)
            .collect::<Result<_, _>>()?,
        round_ends: round_ends.iter().copied().map(event::pos).collect(),
        run_ends: run_ends
            .iter()
            .map(|(pos, end)| wire::RunEnded {
                pos: event::pos(*pos),
                end: event::run_end(end),
            })
            .collect(),
    })
}

fn transcript_item(item: &TranscriptItem<LocatedImage>) -> Result<wire::TranscriptItem, Failure> {
    // The producing Provider generation only decides which plugin its extensions replay to.
    let TranscriptItem {
        pos,
        message: recorded,
        generation: _,
        event: input,
    } = item;
    let message = message(recorded);
    let images = message
        .parts
        .iter()
        .filter_map(|part| match part {
            wire::Part::Image(image) => Some(image.clone()),
            _ => None,
        })
        .collect();
    Ok(wire::TranscriptItem {
        pos: event::pos(*pos),
        event: input
            .as_ref()
            .map(|value| event::event(value, images))
            .transpose()?,
        message,
    })
}

fn message(message: &Message<LocatedImage>) -> wire::Message {
    let Message { role, parts } = message;
    wire::Message {
        role: (*role).into(),
        parts: parts.iter().map(part).collect(),
    }
}

fn part(part: &Part<LocatedImage>) -> wire::Part {
    match part {
        Part::Text { text } => wire::Part::Text(text.clone()),
        // The service's call IDs matter only to the Provider that replays them.
        Part::ToolCall(ToolCall {
            id,
            provider_id: _,
            name,
            arguments,
        }) => wire::Part::ToolCall(wire::Call {
            id: id.to_string(),
            name: name.clone(),
            arguments: arguments.clone(),
        }),
        Part::ToolResult(ToolResult {
            call,
            provider_id: _,
            content,
            is_error,
        }) => wire::Part::ToolResult(wire::CallResult {
            call_id: call.to_string(),
            content: content.clone(),
            is_error: *is_error,
        }),
        // Adapter fields belong to the plugin that wrote them; the composer sees only their size.
        Part::Extension(Extension { data }) => wire::Part::Extension(data.to_string().len() as u64),
        Part::Image(LocatedImage {
            image:
                Image {
                    format,
                    size,
                    content,
                },
            path,
        }) => wire::Part::Image(wire::ImageRef {
            format: (*format).into(),
            size: *size,
            content: content.to_string(),
            path: path.to_string_lossy().into_owned(),
        }),
    }
}

fn capability(id: &CapabilityId) -> wire::CapabilityId {
    let CapabilityId { node, name } = id;
    wire::CapabilityId {
        node: node.to_string(),
        name: name.clone(),
    }
}

fn omission(value: &Omission) -> wire::Omission {
    let Omission { source, reason } = value;
    wire::Omission {
        source: source.clone(),
        reason: reason.clone(),
    }
}

fn budget(value: Budget) -> wire::Budget {
    let Budget {
        context_tokens,
        max_output_tokens,
    } = value;
    wire::Budget {
        context_tokens: context_tokens.get(),
        max_output_tokens: max_output_tokens.get(),
    }
}

fn previous(plan: &ContextPlan) -> wire::Plan {
    let ContextPlan {
        items,
        tools,
        max_output_tokens,
        omitted,
    } = plan;
    wire::Plan {
        items: items
            .iter()
            .map(|item| match item {
                PlanItem::Message { message, sources } => {
                    // An inline message holds only text, which the composer receives joined.
                    let Message { role, parts: _ } = message;
                    wire::PlanItem::Message(wire::Inline {
                        role: (*role).into(),
                        text: message.joined_text(),
                        sources: sources
                            .iter()
                            .map(|Source { id, hash }| wire::Source {
                                id: id.clone(),
                                hash: hash.to_string(),
                            })
                            .collect(),
                    })
                }
                PlanItem::Log { pos, images } => wire::PlanItem::Log(wire::LogRef {
                    pos: event::pos(*pos),
                    images: *images,
                }),
            })
            .collect(),
        tools: tools.iter().map(|(id, _)| capability(id)).collect(),
        max_output_tokens: *max_output_tokens,
        omitted: omitted.iter().map(omission).collect(),
    }
}

pub(super) fn composition(value: wire::Composition) -> Result<Composition, Failure> {
    Ok(match value {
        wire::Composition::Reply(value) => Composition::Plan(plan(value)?),
        wire::Composition::Compact(wire::Compaction { upto, plan: value }) => {
            Composition::Compact {
                upto: log_pos(upto),
                plan: plan(value)?,
            }
        }
    })
}

fn log_pos(value: wire::LogPos) -> LogPos {
    let wire::LogPos { epoch, seq } = value;
    LogPos {
        epoch: Epoch(epoch),
        seq: Seq(seq),
    }
}

fn plan(value: wire::Plan) -> Result<CompositionPlan, Failure> {
    let wire::Plan {
        items,
        tools,
        max_output_tokens,
        omitted,
    } = value;
    Ok(CompositionPlan {
        items: items.into_iter().map(plan_item).collect::<Result<_, _>>()?,
        tools: tools
            .into_iter()
            .map(|wire::CapabilityId { node, name }| {
                Ok(CapabilityId {
                    node: node.parse().map_err(|error| {
                        convert::bad(format!("composer capability node: {error}"))
                    })?,
                    name,
                })
            })
            .collect::<Result<_, Failure>>()?,
        max_output_tokens,
        omitted: omitted
            .into_iter()
            .map(|wire::Omission { source, reason }| Omission { source, reason })
            .collect(),
    })
}

fn plan_item(item: wire::PlanItem) -> Result<CompositionItem, Failure> {
    Ok(match item {
        wire::PlanItem::Message(wire::Inline {
            role,
            text,
            sources,
        }) => CompositionItem::Message {
            role: role.into(),
            text,
            sources: sources
                .into_iter()
                .map(|wire::Source { id, hash }| {
                    Ok(Source {
                        id,
                        hash: hash.parse().map_err(|error| {
                            convert::bad(format!("composer source hash: {error}"))
                        })?,
                    })
                })
                .collect::<Result<_, Failure>>()?,
        },
        wire::PlanItem::Log(wire::LogRef { pos, images }) => CompositionItem::Log {
            pos: log_pos(pos),
            images,
        },
    })
}

#[cfg(test)]
#[path = "composer_convert_tests.rs"]
mod tests;
