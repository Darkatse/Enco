//! Replace one file without exposing partial bytes, preserving its existing permissions.
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub(crate) async fn write(path: &Path, bytes: &[u8]) -> Result<PathBuf, std::io::Error> {
    let (path, permissions) = match tokio::fs::canonicalize(path).await {
        Ok(target) => {
            let metadata = tokio::fs::metadata(&target).await?;
            (target, Some(metadata.permissions()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (path.to_owned(), None),
        Err(error) => return Err(error),
    };
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temp = path.with_extension(format!("enco-tmp-{}", ulid::Ulid::generate()));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .await?;
    let result = async {
        file.write_all(bytes).await?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions).await?;
        }
        file.sync_all().await?;
        tokio::fs::rename(&temp, &path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    result?;
    Ok(path)
}
