//! Doc comments as they read inside a manual.
//!
//! The first paragraph is the summary an index shows on one line. Headings move below the item's
//! own heading, intra-doc links keep only their text (the manual has no pages to link to), and
//! lines rustdoc hides in Rust examples stay hidden.
use std::collections::HashMap;

/// The first paragraph on one line, or an empty string when the item has no docs.
pub(crate) fn summary(docs: Option<&str>, links: &LinkTargets) -> String {
    let first = docs
        .unwrap_or_default()
        .trim()
        .split("\n\n")
        .next()
        .unwrap_or_default();
    unlink(
        &first.lines().map(str::trim).collect::<Vec<_>>().join(" "),
        links,
    )
}

/// Text continued under a list item: lines after the first take `indent`, and blank lines stay
/// empty so that no line ends in spaces.
pub(crate) fn continued(text: &str, indent: &str) -> String {
    text.split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 || line.is_empty() {
                line.to_owned()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The whole doc comment, with its headings moved `depth` levels down.
pub(crate) fn body(docs: Option<&str>, links: &LinkTargets, depth: usize) -> String {
    let mut out = Vec::new();
    let mut fence: Option<bool> = None;
    for line in docs.unwrap_or_default().trim().lines() {
        let trimmed = line.trim_start();
        if let Some(info) = trimmed.strip_prefix("```") {
            fence = match fence {
                None => Some(is_rust(info)),
                Some(_) => None,
            };
            out.push(if fence == Some(true) {
                "```rust".to_owned()
            } else {
                line.to_owned()
            });
            continue;
        }
        match fence {
            Some(true) if trimmed == "#" || trimmed.starts_with("# ") => {}
            Some(_) => out.push(line.to_owned()),
            None if line.starts_with('#') => out.push(format!("{}{line}", "#".repeat(depth))),
            None => out.push(unlink(line, links)),
        }
    }
    out.join("\n")
}

/// rustdoc treats a fence with no language, or with only attributes such as `no_run`, as Rust.
fn is_rust(info: &str) -> bool {
    info.split(',').map(str::trim).all(|word| {
        matches!(
            word,
            "" | "rust" | "ignore" | "no_run" | "should_panic" | "compile_fail" | "edition2024"
        )
    })
}

/// The link texts rustdoc resolved for one item.
pub(crate) type LinkTargets = HashMap<String, rustdoc_types::Id>;

/// Replace `[label]` and `[label](target)` with `label` when rustdoc resolved the link.
fn unlink(line: &str, links: &LinkTargets) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let label = &after[..close];
        let tail = &after[close + 1..];
        let target = tail
            .strip_prefix('(')
            .and_then(|target| target.find(')').map(|end| &target[..end]));
        match target {
            Some(target) if links.contains_key(target) => {
                out.push_str(label);
                rest = &tail[target.len() + 2..];
            }
            None if links.contains_key(label) => {
                out.push_str(label);
                rest = tail;
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::{LinkTargets, body, continued, summary};
    use rustdoc_types::Id;

    #[test]
    fn docs_fit_under_the_item_heading_and_keep_only_what_readers_see() {
        let links: LinkTargets = [("`Thing`".to_owned(), Id(1))].into();
        let docs = "Uses [`Thing`] and [the guide](https://example.org).\n\n\
                    # Errors\n\nFails when [`Thing`](crate::Thing) is absent.\n\n\
                    ```\n# use thing::Thing;\nlet thing = Thing;\n```";
        assert_eq!(
            summary(Some(docs), &links),
            "Uses `Thing` and [the guide](https://example.org)."
        );
        let links: LinkTargets = [
            ("`Thing`".to_owned(), Id(1)),
            ("crate::Thing".to_owned(), Id(1)),
        ]
        .into();
        assert_eq!(
            body(Some(docs), &links, 2),
            "Uses `Thing` and [the guide](https://example.org).\n\n\
             ### Errors\n\nFails when `Thing` is absent.\n\n\
             ```rust\nlet thing = Thing;\n```"
        );
    }

    #[test]
    fn continued_paragraphs_leave_blank_lines_without_trailing_spaces() {
        assert_eq!(
            continued("First.\n\nSecond\nline.", "  "),
            "First.\n\n  Second\n  line."
        );
    }
}
