//! Attachment downloads and SnowLuma's file URL and chunked-byte responses; content supplies
//! only file IDs, never an address to download through the authenticated service.
use crate::{
    channel::{Fault, FetchStep, Response},
    outgoing::FILE_TIMEOUT_MS,
    wire::{self, Envelope},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use enco_sdk::types::Failure;
use serde::Deserialize;
use serde_json::{Value, json};

/// Why a file is unavailable when SnowLuma has no address to download it from.
pub(super) const NO_ADDRESS: &str = "SnowLuma gave no download address for it";

/// Downloads a media URL without credentials; a permanent HTTP failure affects only this file.
pub(super) fn download(url: String, responses: Vec<Response>) -> Result<FetchStep, Fault> {
    let Some(response) = responses.into_iter().next() else {
        return Ok(FetchStep::Download(url));
    };
    let status = response.status;
    let message = format!("file download returned HTTP {status}");
    match status {
        200..=299 => Ok(FetchStep::File(response.body)),
        429 | 500.. => Err(Fault {
            failure: Failure {
                code: format!("channel.http.{status}"),
                message,
                retryable: true,
            },
            unknown: false,
            fatal: false,
            retry_after: None,
        }),
        _ => Ok(FetchStep::Unavailable(message)),
    }
}

/// SnowLuma's answer broke its contract; the host leaves the file unavailable with this message.
fn unreadable(what: &str) -> Fault {
    wire::fault(
        "channel.bad_response",
        format!("Enco could not read the {what} SnowLuma gave"),
    )
}

#[derive(Deserialize)]
struct FileUrl {
    url: String,
}

/// Resolves a file ID, then asks SnowLuma to stream bytes from the address it returned.
pub(super) fn fetch(
    action: &str,
    params: Value,
    responses: Vec<Response>,
) -> Result<FetchStep, Fault> {
    let mut responses = responses.into_iter();
    let Some(response) = responses.next() else {
        return Ok(FetchStep::Request(wire::request(
            action,
            params,
            wire::ACTION_TIMEOUT_MS,
        )));
    };
    if let Some(response) = responses.next() {
        return stream(response);
    }
    let data = match wire::envelope(&response) {
        Ok(Envelope::Ok { data }) => data,
        Ok(Envelope::Failed { wording, .. }) => return Ok(refused(wording)),
        Err(fault) if !fault.fatal && !fault.unknown && !fault.failure.retryable => {
            return Ok(FetchStep::Unavailable(fault.failure.message));
        }
        Err(fault) => return Err(fault),
    };
    let file = FileUrl::deserialize(data).map_err(|_| unreadable("file address"))?;
    if file.url.is_empty() {
        return Ok(FetchStep::Unavailable(NO_ADDRESS.into()));
    }
    Ok(FetchStep::Request(wire::request(
        "download_file_stream",
        json!({"file": file.url}),
        FILE_TIMEOUT_MS,
    )))
}

#[derive(Deserialize)]
#[serde(tag = "data_type")]
enum Frame {
    #[serde(rename = "file_info")]
    Info,
    #[serde(rename = "file_chunk")]
    Chunk { index: usize, data: String },
    #[serde(rename = "file_complete")]
    Complete {
        total_chunks: usize,
        total_bytes: usize,
    },
}

/// Accepts bytes only after the completion frame accounts for every ordered chunk and byte.
fn stream(response: Response) -> Result<FetchStep, Fault> {
    if let Err(fault) = wire::checked(&response) {
        return if !fault.fatal && !fault.failure.retryable {
            Ok(FetchStep::Unavailable(fault.failure.message))
        } else {
            Err(fault)
        };
    }
    let invalid = || unreadable("file stream");
    let text = std::str::from_utf8(&response.body).map_err(|_| invalid())?;
    let mut frames = text
        .split("\r\n\r\n")
        .filter(|frame| !frame.is_empty())
        .peekable();
    let mut bytes = vec![];
    let mut chunk_count = 0;
    while let Some(frame) = frames.next() {
        let data = match serde_json::from_str(frame).map_err(|_| invalid())? {
            Envelope::Ok { data } => data,
            Envelope::Failed { wording, .. } => return Ok(refused(wording)),
        };
        let frame = Frame::deserialize(data).map_err(|_| invalid())?;
        match frame {
            Frame::Info => {}
            Frame::Chunk { index, data } => {
                if index != chunk_count {
                    return Err(invalid());
                }
                bytes.extend(STANDARD.decode(data).map_err(|_| invalid())?);
                chunk_count += 1;
            }
            Frame::Complete {
                total_chunks,
                total_bytes,
            } => {
                if total_chunks != chunk_count
                    || total_bytes != bytes.len()
                    || frames.peek().is_some()
                {
                    return Err(invalid());
                }
                return Ok(FetchStep::File(bytes));
            }
        }
    }
    Err(invalid())
}

/// The step for a file SnowLuma refused to provide, with the `wording` it gave.
fn refused(wording: Option<String>) -> FetchStep {
    FetchStep::Unavailable(match wording {
        Some(wording) => format!("SnowLuma could not provide this file: {wording}"),
        None => "SnowLuma could not provide this file".into(),
    })
}
