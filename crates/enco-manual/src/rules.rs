//! The documentation rules a machine can check; whether the words are good is left to review.
//!
//! A waiver is a problem wherever it is, private modules included. The rules about wording apply to
//! every doc a manual prints: the navigation manual, which shows private items too, decides which.
use crate::{navigation, render::View, text};
use rustdoc_types::{Attribute, Crate};
use std::{cell::RefCell, collections::BTreeSet, path::PathBuf};

/// The longest first paragraph that still reads as one line of an index.
pub const SUMMARY_CHARS: usize = 160;

/// Words that say nothing about an item, left out when comparing a summary with the item's name.
const FILLER: [&str; 24] = [
    "a", "all", "an", "and", "any", "are", "as", "at", "be", "by", "each", "for", "from", "in",
    "is", "it", "its", "of", "on", "one", "or", "that", "the", "to",
];

/// Placeholders for work that belongs in the roadmap or a plan rather than in docs.
const PLACEHOLDERS: [&str; 4] = ["TODO", "FIXME", "XXX", "TBD"];

/// Which rule an item breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The crate root does not turn on `missing_docs`, so the compiler lets undocumented items
    /// pass.
    Unchecked,
    /// The item opts out of `missing_docs`; an item not worth documenting should not be public.
    Waiver,
    /// The first paragraph is longer than [`SUMMARY_CHARS`].
    Long,
    /// The first paragraph only repeats words of the item's name.
    Restates,
    /// The docs leave a placeholder for later work.
    Placeholder,
}

/// One broken rule, with where the item is declared.
#[derive(Debug, Clone)]
pub struct Problem {
    /// The source file, as rustdoc names it.
    pub file: PathBuf,
    /// The line the item starts on.
    pub line: usize,
    /// The item's name.
    pub item: String,
    /// The rule it breaks.
    pub rule: Rule,
    /// What is wrong and what to do, in one sentence.
    pub message: String,
}

pub(crate) fn check(krate: &Crate) -> Vec<Problem> {
    let printed = RefCell::new(BTreeSet::new());
    navigation::render(&View {
        krate,
        navigation: true,
        prefix: "",
        here: None,
        printed: Some(&printed),
    });
    let printed = printed.into_inner();
    let mut problems = Vec::new();
    if let Some(root) = krate.index.get(&krate.root)
        && !enables(&root.attrs)
        && let Some(span) = &root.span
    {
        problems.push(Problem {
            file: span.filename.clone(),
            line: span.begin.0,
            item: root.name.clone().unwrap_or_default(),
            rule: Rule::Unchecked,
            message: "the crate root does not turn on `missing_docs`; add `#![warn(missing_docs)]`"
                .into(),
        });
    }
    for item in krate.index.values().filter(|item| item.crate_id == 0) {
        let Some(span) = &item.span else {
            continue;
        };
        let mut report = |rule, message| {
            problems.push(Problem {
                file: span.filename.clone(),
                line: span.begin.0,
                item: item.name.clone().unwrap_or_default(),
                rule,
                message,
            });
        };
        if let Some(attribute) = waiver(&item.attrs) {
            report(
                Rule::Waiver,
                format!("`{attribute}` waives its docs; document the item or make it private"),
            );
        }
        if let Some(docs) = item.docs.as_ref().filter(|_| printed.contains(&item.id)) {
            let summary = text::summary(Some(docs), &item.links);
            for (rule, message) in breaks(item.name.as_deref(), docs, &summary) {
                report(rule, message);
            }
        }
    }
    problems.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    problems
}

/// The rules a doc comment breaks, given its first paragraph as readers see it, each with what to
/// do about it.
pub(crate) fn breaks(name: Option<&str>, docs: &str, summary: &str) -> Vec<(Rule, String)> {
    let mut broken = Vec::new();
    let length = summary.chars().count();
    if length > SUMMARY_CHARS {
        broken.push((
            Rule::Long,
            format!(
                "the first paragraph has {length} characters; an index shows it on one line, \
                 so keep it within {SUMMARY_CHARS} and move the rest to a later paragraph"
            ),
        ));
    }
    if name.is_some_and(|name| restates(summary, name)) {
        broken.push((
            Rule::Restates,
            "the first paragraph only repeats the name; say what the name does not".into(),
        ));
    }
    if let Some(placeholder) = docs
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|word| PLACEHOLDERS.contains(word))
    {
        broken.push((
            Rule::Placeholder,
            format!("the docs leave `{placeholder}`; record the work in the roadmap or a plan"),
        ));
    }
    broken
}

/// The attribute by which an item opts out of `missing_docs`, if any of its attributes does.
fn waiver(attrs: &[Attribute]) -> Option<&str> {
    missing_docs(attrs)
        .find_map(|(level, attribute)| matches!(level, "allow" | "expect").then_some(attribute))
}

/// Whether one of an item's attributes turns `missing_docs` on.
fn enables(attrs: &[Attribute]) -> bool {
    missing_docs(attrs).any(|(level, _)| matches!(level, "warn" | "deny" | "forbid"))
}

/// Every level an item's own attributes set for `missing_docs`, each with the attribute as rustdoc
/// prints it.
///
/// Each one counts: rustc obeys the last, so a waiver after `warn` still waives.
fn missing_docs(attrs: &[Attribute]) -> impl Iterator<Item = (&str, &str)> {
    attrs.iter().filter_map(|attribute| match attribute {
        Attribute::Other(text) => level(text, "missing_docs").map(|level| (level, text.as_str())),
        _ => None,
    })
}

/// The level an attribute, as rustdoc prints it, sets for `lint`, such as `allow` in
/// `#[allow(missing_docs, reason = "…")]`.
fn level<'a>(attribute: &'a str, lint: &str) -> Option<&'a str> {
    let (level, lints) = attribute.strip_prefix("#[")?.split_once('(')?;
    lints
        .split(',')
        .any(|name| name.trim().trim_end_matches(")]") == lint)
        .then(|| level.trim())
}

/// Whether every meaningful word of `summary` is already a word of `name`.
fn restates(summary: &str, name: &str) -> bool {
    let named = words(name);
    words(summary)
        .iter()
        .filter(|word| !FILLER.contains(&word.as_str()))
        .all(|word| named.contains(word))
}

/// Lowercase words, splitting `snake_case` and `CamelCase` as well as spaces and punctuation.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            words.extend((!current.is_empty()).then(|| std::mem::take(&mut current)));
            previous_lower = false;
            continue;
        }
        if c.is_uppercase() && previous_lower {
            words.push(std::mem::take(&mut current));
        }
        previous_lower = c.is_lowercase() || c.is_ascii_digit();
        current.extend(c.to_lowercase());
    }
    words.extend((!current.is_empty()).then_some(current));
    words
}

#[cfg(test)]
mod tests {
    use super::{enables, level, restates, waiver};
    use rustdoc_types::Attribute;

    #[test]
    fn a_summary_restates_its_name_only_when_it_adds_no_word() {
        assert!(restates("The plugin.", "plugin"));
        assert!(restates("Plugin name.", "PluginName"));
        assert!(!restates("Plugin source directory.", "name"));
        assert!(!restates("The plugin this result is for.", "plugin"));
    }

    #[test]
    fn a_lint_level_is_found_with_a_reason_or_beside_other_lints() {
        let lint = "missing_docs";
        assert_eq!(
            level(r#"#[expect(missing_docs, reason = "x")]"#, lint),
            Some("expect")
        );
        assert_eq!(
            level("#[allow(dead_code, missing_docs)]", lint),
            Some("allow")
        );
        assert_eq!(level("#[warn(missing_docs)]", lint), Some("warn"));
        assert_eq!(
            level(r#"#[expect(dead_code, reason = "missing_docs")]"#, lint),
            None
        );
    }

    #[test]
    fn a_waiver_after_the_lint_is_turned_on_still_counts() {
        let attrs = ["#[warn(missing_docs)]", "#[allow(missing_docs)]"]
            .map(|text| Attribute::Other(text.into()));
        assert!(enables(&attrs));
        assert_eq!(waiver(&attrs), Some("#[allow(missing_docs)]"));
    }
}
