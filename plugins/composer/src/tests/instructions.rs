use super::*;

#[test]
fn owner_and_profile_instructions_have_independent_budgets_and_keep_their_order() {
    let mut input = input();
    input.owner_instructions = Some("owner ".repeat(3000));
    input.profile_instructions = Some("profile ".repeat(3000));
    let plan = reply(&input);
    assert!(plan.omitted.is_empty());
    let sources: Vec<_> = plan
        .items
        .iter()
        .filter_map(|item| match item {
            PlanItem::Message(message) => Some(&message.sources),
            _ => None,
        })
        .flatten()
        .map(|source| source.id.as_str())
        .collect();
    assert_eq!(
        sources,
        ["instructions:AGENTS.md", "instructions:profiles/default.md"]
    );
}

#[test]
fn sessions_without_the_owners_context_are_not_told_where_the_owner_instructions_are() {
    let mut input = input();
    assert!(text(&reply(&input)).contains("/home/enco/AGENTS.md"));
    input.owner_context = false;
    let text = text(&reply(&input));
    assert!(!text.contains("AGENTS.md"));
    assert!(text.contains("/home/enco/profiles/default.md"));
}
