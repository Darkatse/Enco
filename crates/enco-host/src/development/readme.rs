use super::DevelopmentError;
use enco_kernel::Loaded;
use std::{fmt::Write, path::Path};

const BEGIN: &str = "<!-- enco:generated:begin -->";
const END: &str = "<!-- enco:generated:end -->";

pub(super) async fn write(directory: &Path, loaded: &Loaded) -> Result<(), DevelopmentError> {
    let path = directory.join("README.md");
    let previous = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut generated = format!(
        "{BEGIN}\n\n{}\n\n## Configuration\n\n```json\n{}\n```\n",
        loaded.summary,
        serde_json::to_string_pretty(&loaded.config_schema)?
    );
    if !loaded.credentials.is_empty() {
        generated
            .push_str("\n## Credentials\n\n| Name | Destination | Required |\n|---|---|---|\n");
        for credential in &loaded.credentials {
            writeln!(
                generated,
                "| `{}` | {} | {} |",
                credential.name,
                credential
                    .origin
                    .as_deref()
                    .unwrap_or("Caller-selected service"),
                credential.required
            )?;
        }
    }
    if let Some(tools) = &loaded.tools {
        for tool in &tools.specs {
            writeln!(
                generated,
                "\n## `{}`\n\n{}\n\n```json\n{}\n```",
                tool.name,
                tool.description,
                serde_json::to_string_pretty(&tool.input_schema)?
            )?;
        }
    }
    writeln!(generated, "\n{END}")?;
    let text = match (previous.find(BEGIN), previous.find(END)) {
        (Some(begin), Some(end)) if begin < end => format!(
            "{}{}{}",
            &previous[..begin],
            generated.trim_end(),
            &previous[end + END.len()..]
        ),
        (None, None) => format!("{generated}{previous}"),
        _ => {
            return Err(DevelopmentError::Build(format!(
                "{}: generated README markers are incomplete or out of order",
                path.display()
            )));
        }
    };
    if text != previous {
        tokio::fs::write(path, text).await?;
    }
    Ok(())
}

/// Only the generated section may change during a build.
pub(super) fn authored(text: &str) -> Result<String, DevelopmentError> {
    match (text.find(BEGIN), text.find(END)) {
        (Some(begin), Some(end)) if begin < end => {
            let suffix = &text[end + END.len()..];
            let suffix = suffix.strip_prefix('\n').unwrap_or(suffix);
            Ok(format!("{}{}", &text[..begin], suffix))
        }
        (None, None) => Ok(text.to_owned()),
        _ => Err(DevelopmentError::Build(
            "README generated markers are incomplete or out of order".into(),
        )),
    }
}
