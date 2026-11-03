use crate::{IMAGE_MAX_BYTES, Kernel, ProviderRequest, Store, StoreError};
use enco_core::*;

/// Why bytes are not an image a model may be shown.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ImageError {
    /// The bytes have no supported image signature.
    #[error("unsupported image format; use PNG, JPEG or WebP")]
    Unsupported,
    /// The original bytes exceed the ingestion limit.
    #[error("image has {size} bytes; the limit is {IMAGE_MAX_BYTES} bytes")]
    TooLarge {
        /// Original byte length.
        size: u64,
    },
}

impl Kernel {
    /// Store an attachment as an image when its bytes are an image a model may be shown, or as a
    /// file with `name` otherwise.
    pub async fn store_attachment(
        &self,
        name: String,
        bytes: &[u8],
    ) -> Result<Attachment, StoreError> {
        let size = bytes.len() as u64;
        let content = self.deps.store.put_blob(bytes).await?;
        Ok(match accepted(bytes) {
            Ok(format) => Attachment::Image(Image {
                format,
                size,
                content,
            }),
            Err(_) => Attachment::File(File {
                name,
                size,
                content,
            }),
        })
    }
}

/// Store bytes that are an image a model may be shown; the inner error says why they are not.
pub(crate) async fn store(
    store: &dyn Store,
    bytes: &[u8],
) -> Result<Result<Image, ImageError>, StoreError> {
    let format = match accepted(bytes) {
        Ok(format) => format,
        Err(rejected) => return Ok(Err(rejected)),
    };
    let content = store.put_blob(bytes).await?;
    Ok(Ok(Image {
        format,
        size: bytes.len() as u64,
        content,
    }))
}

/// The bytes' image format, or why they are not an image a model may be shown.
fn accepted(bytes: &[u8]) -> Result<ImageFormat, ImageError> {
    let size = bytes.len() as u64;
    if size > IMAGE_MAX_BYTES {
        return Err(ImageError::TooLarge { size });
    }
    ImageFormat::detect(bytes).ok_or(ImageError::Unsupported)
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
