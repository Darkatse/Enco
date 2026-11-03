use crate::ContentHash;
use serde::{Deserialize, Serialize};

/// Non-text input accepted by a completion endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    /// Still-image input.
    Image,
}

/// Encodings shared by the supported image input protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    /// PNG encoding.
    Png,
    /// JPEG encoding.
    Jpeg,
    /// WebP encoding.
    Webp,
}

impl ImageFormat {
    /// Recognize an encoding by its file signature, independently of filenames and MIME labels.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some(Self::Jpeg)
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            Some(Self::Webp)
        } else {
            None
        }
    }

    /// MIME label used at protocol boundaries.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

/// Recorded image metadata; the kernel stores the bytes before accepting this reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    /// Detected encoding.
    pub format: ImageFormat,
    /// Original byte length, before protocol encoding.
    pub size: u64,
    /// Address of the original bytes in the blob store.
    pub content: ContentHash,
}

/// Bytes resolved from a recorded image for one frozen Provider request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageData {
    /// Recorded encoding.
    pub format: ImageFormat,
    /// Original bytes, encoded only by the Provider.
    pub data: Vec<u8>,
}

/// Part of a message's content kept only as a description, such as a location or an attachment
/// that could not be fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unviewable {
    /// A plain description such as report.pdf, voice message or location, not a protocol type
    /// name.
    pub what: String,
    /// Why it cannot be shown.
    pub reason: UnviewableReason,
}

/// Why part of a message is kept only as a description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnviewableReason {
    /// It is not a file Enco can keep, such as a location or a poll.
    Unsupported,
    /// It could not be retrieved, for this reason.
    Unavailable(String),
}

/// A recorded attachment: an image a model may view, or any other file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Attachment {
    /// Bytes in an accepted image format, within the image limit.
    Image(Image),
    /// Any other file.
    File(crate::File),
}

impl Attachment {
    /// The recorded image, when this attachment is one.
    pub fn image(&self) -> Option<&Image> {
        match self {
            Self::Image(image) => Some(image),
            Self::File(_) => None,
        }
    }

    /// The address of its bytes in the blob store.
    pub fn content(&self) -> ContentHash {
        match self {
            Self::Image(image) => image.content,
            Self::File(file) => file.content,
        }
    }
}
