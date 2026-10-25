//! Markdown manuals rendered from rustdoc's JSON output.
//!
//! An API manual says what a library offers its callers, by the paths they write. It names source
//! files but not lines, so only an interface change alters it. A navigation manual covers every
//! module and item, private ones included, with source lines, to show where to change what.
#![warn(missing_docs)]
mod api;
mod navigation;
mod render;
mod signature;
mod text;

use render::View;
pub use rustdoc_types::{Crate, FORMAT_VERSION};
use serde::Deserialize;
use std::collections::BTreeMap;

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
        private: false,
        lines: false,
        prefix: sources,
        here: None,
    };
    api::render(&view)
}

/// The navigation manual of a crate, from rustdoc JSON that includes private items: file names
/// mapped to Markdown, `README.md` first among them, with source lines relative to `sources`.
pub fn navigation(krate: &Crate, sources: &str) -> BTreeMap<String, String> {
    let view = View {
        krate,
        private: true,
        lines: true,
        prefix: sources,
        here: None,
    };
    navigation::render(&view)
}
