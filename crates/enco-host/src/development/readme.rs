//! The generated section of a plugin's README.
//!
//! A build rewrites only the text between the `enco:generated` begin and end markers, adding the
//! section at the top when both are missing; a lone or misordered marker fails the build. The rest
//! of the file belongs to its author.
use super::{
    DevelopmentError, EntryError, EntryProblem,
    error::{entry, io},
};
use enco_kernel::Loaded;
use std::path::Path;

pub(super) const FILE: &str = "README.md";
const BEGIN: &str = "<!-- enco:generated:begin -->";
const END: &str = "<!-- enco:generated:end -->";

pub(super) async fn write(directory: &Path, loaded: &Loaded) -> Result<(), DevelopmentError> {
    let path = directory.join(FILE);
    let previous = match tokio::fs::read(&path).await {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| entry(&path, EntryProblem::NotText))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io(&path, &error).into()),
    };
    let generated = format!("{BEGIN}\n\n{}\n{END}\n", runtime(loaded)?);
    let text = match (previous.find(BEGIN), previous.find(END)) {
        (Some(begin), Some(end)) if begin < end => format!(
            "{}{}{}",
            &previous[..begin],
            generated.trim_end(),
            &previous[end + END.len()..]
        ),
        (None, None) => format!("{generated}{previous}"),
        _ => return Err(entry(&path, EntryProblem::ReadmeMarkers).into()),
    };
    if text != previous {
        tokio::fs::write(&path, text)
            .await
            .map_err(|error| io(&path, &error))?;
    }
    Ok(())
}

/// Only the generated section may change during a build.
pub(super) fn authored(path: &Path, text: &str) -> Result<String, EntryError> {
    match (text.find(BEGIN), text.find(END)) {
        (Some(begin), Some(end)) if begin < end => {
            let suffix = &text[end + END.len()..];
            let suffix = suffix.strip_prefix('\n').unwrap_or(suffix);
            Ok(format!("{}{}", &text[..begin], suffix))
        }
        (None, None) => Ok(text.to_owned()),
        _ => Err(entry(path, EntryProblem::ReadmeMarkers)),
    }
}

/// What a loaded generation declares, as its README's generated section and its manual show it.
///
/// That is its summary, the configuration it accepts, its credentials, and an index of its tools
/// followed by each tool's description and arguments.
pub(super) fn runtime(loaded: &Loaded) -> Result<String, DevelopmentError> {
    let mut out = format!(
        "{}\n\n## Configuration\n\n```json\n{}\n```\n",
        loaded.summary,
        schema(&loaded.config_schema)?
    );
    if !loaded.credentials.is_empty() {
        out.push_str("\n## Credentials\n\n| Name | Destination | Required |\n|---|---|---|\n");
        for credential in &loaded.credentials {
            out.push_str(&format!(
                "| `{}` | {} | {} |\n",
                credential.name,
                credential
                    .origin
                    .as_deref()
                    .unwrap_or("Caller-selected service"),
                credential.required
            ));
        }
    }
    let tools = loaded.tools.as_ref().map_or(&[][..], |tools| &tools.specs);
    if !tools.is_empty() {
        out.push_str("\n## Tools\n\n");
        for tool in tools {
            out.push_str(&format!(
                "- `{}` — {}\n",
                tool.name,
                paragraph(&tool.description)
            ));
        }
        for tool in tools {
            out.push_str(&format!(
                "\n### `{}`\n\n{}\n\n```json\n{}\n```\n",
                tool.name,
                tool.description,
                schema(&tool.input_schema.to_value())?
            ));
        }
    }
    Ok(out)
}

/// The first paragraph of `text`, on one line.
pub(super) fn paragraph(text: &str) -> String {
    text.trim()
        .split("\n\n")
        .next()
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

fn schema(value: &serde_json::Value) -> Result<String, DevelopmentError> {
    serde_json::to_string_pretty(value).map_err(|error| DevelopmentError::Host(error.to_string()))
}
