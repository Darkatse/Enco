use crate::ContentHash;
use serde::{Deserialize, Serialize};

/// Content addressed to the owner: Markdown text and at most one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outgoing<F = OutgoingFile> {
    /// Markdown authored by the sender.
    pub text: String,
    /// Optional attachment, recorded by reference or carried as bytes at a boundary.
    pub file: Option<F>,
}

/// A recorded file; the kernel stores the bytes before accepting this reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingFile {
    /// Filename without its original directory.
    pub name: String,
    /// Original byte length.
    pub size: u64,
    /// Address of the original bytes in the blob store.
    pub content: ContentHash,
}

/// File bytes handed to the kernel at settlement, or resolved for one delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileData {
    /// Filename without its original directory.
    pub name: String,
    /// Original bytes, encoded only by the channel mapper.
    pub data: Vec<u8>,
}
