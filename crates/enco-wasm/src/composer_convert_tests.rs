//! Checks parsing of untrusted identities returned in a guest plan.
use super::{composition, wire};
use enco_core::{ContentHash, NodeId, code};

fn reply(hash: String, node: String) -> wire::Composition {
    wire::Composition::Reply(wire::Plan {
        items: vec![wire::PlanItem::Message(wire::Inline {
            role: crate::bindings::enco::plugin::types::Role::System,
            text: "instructions".into(),
            sources: vec![wire::Source {
                id: "instructions:AGENTS.md".into(),
                hash,
            }],
        })],
        tools: vec![wire::CapabilityId {
            node,
            name: "fs_read".into(),
        }],
        max_output_tokens: None,
        omitted: vec![],
    })
}

#[test]
fn malformed_guest_identities_are_contract_failures() {
    let bad_hash = composition(reply("invalid".into(), NodeId::new().to_string()));
    assert!(
        matches!(bad_hash, Err(failure) if failure.code == code::PLUGIN_CONTRACT && failure.message.contains("source hash"))
    );
    let bad_node = composition(reply(
        ContentHash::of(b"source").to_string(),
        "invalid".into(),
    ));
    assert!(
        matches!(bad_node, Err(failure) if failure.code == code::PLUGIN_CONTRACT && failure.message.contains("capability node"))
    );
}
