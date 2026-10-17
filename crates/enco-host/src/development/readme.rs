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
    let mut generated = format!(
        "{BEGIN}\n\n{}\n\n## Configuration\n\n```json\n{}\n```\n",
        loaded.summary,
        schema(&loaded.config_schema)?
    );
    if !loaded.credentials.is_empty() {
        generated
            .push_str("\n## Credentials\n\n| Name | Destination | Required |\n|---|---|---|\n");
        for credential in &loaded.credentials {
            generated.push_str(&format!(
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
    if let Some(tools) = &loaded.tools {
        for tool in &tools.specs {
            generated.push_str(&format!(
                "\n## `{}`\n\n{}\n\n```json\n{}\n```\n",
                tool.name,
                tool.description,
                schema(&tool.input_schema)?
            ));
        }
    }
    generated.push_str(&format!("\n{END}\n"));
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

fn schema(value: &serde_json::Value) -> Result<String, DevelopmentError> {
    serde_json::to_string_pretty(value).map_err(|error| DevelopmentError::Host(error.to_string()))
}
