use crate::{IMAGE_MAX_BYTES, Kernel, ProviderRequest, Store, StoreError};
use enco_core::*;

/// Image ingestion either rejects its input or reports a storage failure.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    /// The bytes have no supported image signature.
    #[error("unsupported image format; use PNG, JPEG or WebP")]
    Unsupported,
    /// The original bytes exceed the ingestion limit.
    #[error("image has {size} bytes; the limit is {IMAGE_MAX_BYTES} bytes")]
    TooLarge {
        /// Original byte length.
        size: u64,
    },
    /// The content store could not accept the image.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl Kernel {
    /// Store image bytes through the same ingestion boundary used by tool settlement.
    pub async fn store_image(&self, bytes: &[u8]) -> Result<Image, ImageError> {
        store(self.deps.store.as_ref(), bytes).await
    }
}

pub(crate) async fn store(store: &dyn Store, bytes: &[u8]) -> Result<Image, ImageError> {
    let size = bytes.len() as u64;
    if size > IMAGE_MAX_BYTES {
        return Err(ImageError::TooLarge { size });
    }
    let format = ImageFormat::detect(bytes).ok_or(ImageError::Unsupported)?;
    let content = store.put_blob(bytes).await?;
    Ok(Image {
        format,
        size,
        content,
    })
}

/// Resolve bytes once when freezing a plan; retries retain this same request value.
pub(crate) async fn resolve(
    request: ProviderRequest<Image>,
    store: &dyn Store,
) -> Result<ProviderRequest, StoreError> {
    let mut messages = Vec::with_capacity(request.messages.len());
    for message in request.messages {
        let mut parts = Vec::with_capacity(message.parts.len());
        for part in message.parts {
            parts.push(match part.retype() {
                Ok(part) => part,
                Err(image) => Part::Image(ImageData {
                    format: image.format,
                    data: store.get_blob(&image.content).await?,
                }),
            });
        }
        messages.push(Message {
            role: message.role,
            parts,
        });
    }
    Ok(ProviderRequest {
        messages,
        tools: request.tools,
        max_output_tokens: request.max_output_tokens,
    })
}
