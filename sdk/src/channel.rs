//! What every channel mapper shares: the activity card's wording and `multipart/form-data` uploads.
//!
//! Each protocol lays the card out with its own components; the words on it are the same
//! everywhere. [`export!`](crate::export) gives a channel plugin's `activity` these same types, so
//! name them from here.
pub use crate::bindings::exports::enco::plugin::channel::{ActivityView, ToolActivity, ToolState};

/// The label of the activity card's button that cancels the Run.
pub const CANCEL_LABEL: &str = "Cancel";

impl ActivityView {
    /// The activity card's text: `Working` and the elapsed `m:ss`, then a line per tool.
    ///
    /// Each tool line is a mark and the tool's name; the marks are `…` running, `✓` ok,
    /// `✗` failed and `?` unknown.
    pub fn text(&self) -> String {
        let mut text = format!(
            "Working {}:{:02}",
            self.elapsed_seconds / 60,
            self.elapsed_seconds % 60
        );
        for tool in &self.tools {
            let mark = match tool.state {
                ToolState::Running => "…",
                ToolState::Ok => "✓",
                ToolState::Failed => "✗",
                ToolState::Unknown => "?",
            };
            text.push_str(&format!("\n{mark} {}", tool.name));
        }
        text
    }
}

/// One part of a `multipart/form-data` body.
pub struct FormPart<'a> {
    /// The form field's name.
    pub name: &'a str,
    /// The uploaded file's name, sent with `\r`, `\n` and `"` percent-encoded; absent for a field
    /// that is not a file.
    pub filename: Option<&'a str>,
    /// The part's `Content-Type`; absent leaves the receiver's default, `text/plain`.
    pub content_type: Option<&'a str>,
    /// The part's bytes, sent as they are.
    pub data: &'a [u8],
}

/// Encode `parts` in order as `multipart/form-data`, returning the `content-type` header value and
/// the body.
///
/// The boundary occurs in no part's name, file name or bytes.
pub fn multipart(parts: &[FormPart<'_>]) -> (String, Vec<u8>) {
    let mut boundary = String::from("enco-boundary");
    while parts.iter().any(|part| {
        [
            part.name.as_bytes(),
            part.filename.unwrap_or_default().as_bytes(),
            part.data,
        ]
        .into_iter()
        .any(|bytes| {
            bytes
                .windows(boundary.len())
                .any(|window| window == boundary.as_bytes())
        })
    }) {
        boundary.push('_');
    }
    let mut body = Vec::new();
    for part in parts {
        let mut head = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"",
            part.name
        );
        if let Some(filename) = part.filename {
            let filename = filename
                .replace('\r', "%0D")
                .replace('\n', "%0A")
                .replace('"', "%22");
            head.push_str(&format!("; filename=\"{filename}\""));
        }
        head.push_str("\r\n");
        if let Some(content_type) = part.content_type {
            head.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        head.push_str("\r\n");
        body.extend_from_slice(head.as_bytes());
        body.extend_from_slice(part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_boundary_delimits_only_the_parts() {
        let file = b"\0enco-boundary\r\n--enco-boundary_";
        let (content_type, body) = multipart(&[
            FormPart {
                name: "payload",
                filename: None,
                content_type: Some("application/json"),
                data: b"{}",
            },
            FormPart {
                name: "file",
                filename: Some("plot.bin"),
                content_type: Some("application/octet-stream"),
                data: file,
            },
        ]);
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .expect("multipart content type");
        let delimiter = format!("\r\n--{boundary}");
        let delimiters = body
            .windows(delimiter.len())
            .filter(|window| *window == delimiter.as_bytes())
            .count();
        // One before the second part and one closing the body: the file bytes add none.
        assert_eq!(delimiters, 2);
        assert!(body.windows(file.len()).any(|window| window == file));
    }
}
