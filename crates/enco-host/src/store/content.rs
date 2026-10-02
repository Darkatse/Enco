//! Shared content-addressed IO for removable blobs and retained component artifacts.
use super::backend;
use enco_core::ContentHash;
use enco_kernel::StoreError;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub(super) async fn put(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    match tokio::fs::read(path).await {
        Ok(existing) if existing == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(backend(format!("reading {}: {error}", path.display()))),
    }
    let parent = path
        .parent()
        .ok_or_else(|| backend("content path has no parent"))?;
    tokio::fs::create_dir_all(parent).await.map_err(backend)?;
    let temp = path.with_extension(format!("tmp.{}", ulid::Ulid::generate()));
    let mut file = tokio::fs::File::create(&temp).await.map_err(backend)?;
    file.write_all(bytes).await.map_err(backend)?;
    file.sync_all().await.map_err(backend)?;
    tokio::fs::rename(temp, path).await.map_err(backend)
}

pub(super) async fn get(
    path: &Path,
    hash: &ContentHash,
    missing: fn(ContentHash) -> StoreError,
) -> Result<Vec<u8>, StoreError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => missing(*hash),
            _ => backend(format!("reading {}: {error}", path.display())),
        })?;
    if ContentHash::of(&bytes) != *hash {
        return Err(missing(*hash));
    }
    Ok(bytes)
}
