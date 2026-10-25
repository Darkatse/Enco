//! Where a blob lives on disk, by its content address.
use enco_core::ContentHash;
use std::path::{Path, PathBuf};

/// Address a blob without opening the store.
pub fn blob_path(blobs: &Path, hash: &ContentHash) -> PathBuf {
    let address = hash.to_string();
    blobs.join(&address[..2]).join(address)
}
