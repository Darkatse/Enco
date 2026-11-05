//! Mapper evidence in the database: each mapping record and each response it read, kept
//! zstd-compressed and addressed by its uncompressed bytes.
//!
//! Unlike a blob, evidence has no file that the Agent reads by path, so it can be compressed and
//! many small records do not each take a filesystem block.
use super::backend;
use enco_core::ContentHash;
use enco_kernel::StoreError;
use rusqlite::{Connection, OptionalExtension, params};

/// Store `bytes` under their address once; storing the same bytes again changes nothing.
pub(super) fn put(connection: &Connection, bytes: &[u8]) -> Result<ContentHash, StoreError> {
    let hash = ContentHash::of(bytes);
    let stored: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evidence WHERE hash=?)",
            [hash.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if !stored {
        connection
            .execute(
                "INSERT INTO evidence(hash,body) VALUES (?,?)",
                params![hash.to_string(), compress(bytes)?],
            )
            .map_err(backend)?;
    }
    Ok(hash)
}

/// The evidence stored under `hash`, checked against its address.
pub(super) fn get(connection: &Connection, hash: ContentHash) -> Result<Vec<u8>, StoreError> {
    let body: Vec<u8> = connection
        .query_row(
            "SELECT body FROM evidence WHERE hash=?",
            [hash.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?
        .ok_or(StoreError::Evidence(hash))?;
    let bytes = decompress(&body).map_err(|_| StoreError::Evidence(hash))?;
    if ContentHash::of(&bytes) != hash {
        return Err(StoreError::Evidence(hash));
    }
    Ok(bytes)
}

/// zstd at level 3, the framing of every compressed column.
pub(super) fn compress(bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    zstd::stream::encode_all(bytes, 3).map_err(backend)
}

/// The bytes [`compress`] framed.
pub(super) fn decompress(bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    zstd::stream::decode_all(bytes).map_err(backend)
}
