//! Checks what a Provider's answer must hold before the kernel accepts it.
use super::{complete_error, completion, wire, wit};
use enco_core::*;

fn answer(parts: Vec<wit::Part>) -> Result<enco_kernel::Completion, Failure> {
    completion(wire::Completion {
        message: wit::Message {
            role: wit::Role::Assistant,
            parts,
        },
        usage: wit::Usage {
            input_tokens: 1,
            output_tokens: 1,
            cached_input_tokens: None,
        },
        stop_reason: wire::StopReason::EndTurn,
    })
}

fn extension(data: serde_json::Value) -> wit::Part {
    wit::Part::Extension(wit::Extension {
        data: data.to_string(),
    })
}

fn breaks_the_contract<T>(result: Result<T, Failure>) -> bool {
    matches!(result, Err(failure) if failure.code == code::PLUGIN_CONTRACT)
}

#[test]
fn an_extension_holds_the_whole_message_it_comes_with() {
    let text = wit::Part::Text("The files are listed.".into());
    let call = wit::Part::ToolCall(wit::ToolCall {
        id: String::new(),
        name: "fs_list".into(),
        arguments: r#"{"path":"."}"#.into(),
    });
    // Gemini keeps the content it answered with, signatures on the parts they belong to.
    let gemini = extension(serde_json::json!({"role": "model", "parts": [
        {"text": "The files are listed.", "thoughtSignature": "c2ln"},
        {"functionCall": {"name": "fs_list", "args": {"path": "."}}},
    ]}));
    assert!(answer(vec![text.clone(), call.clone(), gemini]).is_ok());
    // Chat Completions keeps the message with the fields the contract has no place for.
    let chat = extension(serde_json::json!({
        "role": "assistant",
        "content": "The files are listed.",
        "reasoning_content": "The owner asked for a listing.",
    }));
    assert!(answer(vec![text.clone(), chat.clone()]).is_ok());

    assert!(breaks_the_contract(answer(vec![
        text.clone(),
        chat.clone(),
        chat
    ])));
    let partial = extension(serde_json::json!({"reasoning_content": "short"}));
    assert!(breaks_the_contract(answer(vec![text, call, partial])));
}

#[test]
fn an_overflow_cannot_count_less_than_the_service_accepts() {
    let overflow = |counted, accepted| {
        complete_error(wire::CompleteError::Overflow(wit::Overflow {
            counted: Some(counted),
            accepted: Some(accepted),
            message: "too long".into(),
        }))
    };
    assert!(overflow(1_644_003, 1_040_384).is_ok());
    assert!(breaks_the_contract(overflow(1_040_384, 1_040_384)));
}
