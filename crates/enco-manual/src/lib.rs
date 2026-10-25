//! Markdown manuals rendered from rustdoc's JSON output.
//!
//! An API manual says what a library offers its callers, by the paths they write. It names source
//! files but not lines, so only an interface change alters it. A navigation manual covers every
//! module and item, private ones included, with source lines, to show where to change what.
#![warn(missing_docs)]
mod api;
mod navigation;
mod render;
mod rules;
mod signature;
mod text;
mod traits;
mod types;

use render::View;
pub use rules::{Problem, Rule, SUMMARY_CHARS};
pub use rustdoc_types::{Crate, FORMAT_VERSION};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The rustdoc flags that write the JSON the manuals read.
///
/// The JSON holds every item, private and hidden ones included: each manual chooses what it shows,
/// and the documentation rules see everything, including a waiver on a private module.
pub const RUSTDOCFLAGS: &str =
    "-Z unstable-options --output-format json --document-private-items --document-hidden-items";

/// Why rustdoc's output could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The output is not rustdoc JSON.
    #[error("rustdoc JSON is malformed: {0}")]
    Json(#[from] serde_json::Error),
    /// A toolchain other than the pinned one wrote it.
    #[error(
        "rustdoc wrote JSON format {found}, but manuals read format {FORMAT_VERSION}; \
         upgrade rustdoc-types together with the toolchain"
    )]
    Format {
        /// The format the file declares.
        found: u32,
    },
}

/// Read rustdoc's JSON output for one crate.
pub fn read(json: &[u8]) -> Result<Crate, ReadError> {
    // A newer format may not parse as this one, so its version is checked first.
    #[derive(Deserialize)]
    struct Version {
        format_version: u32,
    }
    let Version { format_version } = serde_json::from_slice(json)?;
    if format_version != FORMAT_VERSION {
        return Err(ReadError::Format {
            found: format_version,
        });
    }
    Ok(serde_json::from_slice(json)?)
}

/// The API manual of a library: its public items by the paths callers write, with the source file
/// each comes from, relative to `sources`.
pub fn api(krate: &Crate, sources: &str) -> String {
    let view = View {
        krate,
        navigation: false,
        prefix: sources,
        here: None,
        printed: None,
    };
    api::render(&view)
}

/// The navigation manual of a crate, as Markdown files by name, `README.md` first.
///
/// It needs rustdoc JSON that includes private items. Source lines are relative to `sources`.
pub fn navigation(krate: &Crate, sources: &str) -> BTreeMap<String, String> {
    let view = View {
        krate,
        navigation: true,
        prefix: sources,
        here: None,
        printed: None,
    };
    navigation::render(&view)
}

/// The documentation rules a crate's items break, ordered by file and line.
pub fn problems(krate: &Crate) -> Vec<Problem> {
    rules::check(krate)
}

/// The rules one doc comment breaks, for docs written outside Rust such as the WIT's.
///
/// `name` is the documented item's name. Each broken rule comes with what to do about it.
pub fn doc_problems(name: &str, docs: &str) -> Vec<(Rule, String)> {
    rules::breaks(
        Some(name),
        docs,
        &text::summary(Some(docs), &Default::default()),
    )
}
