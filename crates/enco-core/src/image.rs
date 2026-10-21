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

/// An attachment the owner sent but this host could not make visible as an image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unviewable {
    /// Protocol-provided description of the attachment.
    pub what: String,
    /// Recorded reason it was not accepted as an image.
    pub reason: UnviewableReason,
}

/// Expected attachment failures remain part of their input Event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnviewableReason {
    /// The attachment is not a supported image encoding.
    Unsupported,
    /// Its original bytes exceed the image limit.
    TooLarge,
    /// The service permanently refused retrieval, with its diagnostic context.
    Unavailable(String),
}
