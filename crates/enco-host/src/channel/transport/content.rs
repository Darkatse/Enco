//! Reads an input's images and quoted messages through its mapper; earlier context fetches nothing.
use super::*;
use enco_kernel::{ChannelAttachment, ChannelQuote, ChannelQuoted, ChannelQuotedMessage, Fetched};

/// The owner and this account, against which quoted senders are judged as the quote arrives.
#[derive(Clone, Copy)]
pub(super) struct Identities<'a> {
    pub owner: &'a str,
    pub account: &'a str,
}

impl Identities<'_> {
    /// Whether this sender is the configured owner.
    pub(super) fn is_owner(self, sender: Option<&str>) -> bool {
        sender == Some(self.owner)
    }

    /// The message with these images in place of its protocol attachments.
    fn quoted<A>(self, message: ChannelQuotedMessage, images: Vec<A>) -> QuotedMessage<A> {
        let ChannelQuotedMessage {
            depth,
            sender,
            sender_name,
            conversation_name,
            sent_at,
            text,
            images: _,
            unviewable,
        } = message;
        QuotedMessage {
            depth,
            owner: self.is_owner(sender.as_deref()),
            yours: sender.as_deref() == Some(self.account),
            sender,
            sender_name,
            conversation_name,
            sent_at,
            text,
            images,
            unviewable,
        }
    }
}

/// Earlier context keeps what the protocol carried, with images as descriptions.
pub(super) fn earlier(quote: ChannelQuote, identities: Identities<'_>) -> EarlierQuote {
    let ChannelQuote { kind, messages } = quote;
    let messages = match messages {
        ChannelQuoted::Fetch(_) => None,
        ChannelQuoted::Unavailable(reason) => Some(Quoted::Unavailable(reason)),
        ChannelQuoted::Messages(messages) => Some(Quoted::Messages(
            messages
                .into_iter()
                .map(|mut message| {
                    let images = std::mem::take(&mut message.images);
                    let images = images.into_iter().map(|image| image.what).collect();
                    identities.quoted(message, images)
                })
                .collect(),
        )),
    };
    EarlierQuote { kind, messages }
}

impl Reader {
    /// Store the images of a message that may drive a Session, and of the messages it quotes.
    pub(super) async fn content(
        &self,
        content: ChannelContent<ChannelAttachment>,
        identities: Identities<'_>,
    ) -> Result<ChannelContent<Image, Quote>, ChannelError> {
        let ChannelContent {
            text,
            images,
            mut unviewable,
            quote,
        } = content;
        let quote = match quote {
            Some(quote) => Some(self.quote(quote, identities).await?),
            None => None,
        };
        Ok(ChannelContent {
            text,
            images: self.pictures(images, &mut unviewable).await?,
            unviewable,
            quote,
        })
    }

    async fn quote(
        &self,
        quote: ChannelQuote,
        identities: Identities<'_>,
    ) -> Result<Quote, ChannelError> {
        let ChannelQuote { kind, messages } = quote;
        let (messages, fetched) = match messages {
            ChannelQuoted::Messages(messages) => (Ok(messages), None),
            ChannelQuoted::Unavailable(reason) => (Err(reason), None),
            ChannelQuoted::Fetch(locator) => {
                let Fetched { content, origin } =
                    self.read(|| self.adapter.fetch_quote(&locator)).await?;
                (content, origin)
            }
        };
        let messages = match messages {
            Err(reason) => Quoted::Unavailable(reason),
            Ok(messages) => {
                let mut prepared = Vec::with_capacity(messages.len());
                for mut message in messages {
                    let images = std::mem::take(&mut message.images);
                    let images = self.pictures(images, &mut message.unviewable).await?;
                    prepared.push(identities.quoted(message, images));
                }
                Quoted::Messages(prepared)
            }
        };
        Ok(Quote {
            kind,
            messages,
            fetched,
        })
    }

    async fn pictures(
        &self,
        images: Vec<ChannelAttachment>,
        unviewable: &mut Vec<Unviewable>,
    ) -> Result<Vec<Image>, ChannelError> {
        let mut prepared = Vec::with_capacity(images.len());
        for ChannelAttachment { what, locator } in images {
            let fetched = self.read(|| self.adapter.fetch_image(&locator)).await?;
            let reason = match fetched.content {
                Err(reason) => UnviewableReason::Unavailable(reason),
                Ok(bytes) => match self.kernel.store_image(&bytes).await {
                    Ok(image) => {
                        prepared.push(image);
                        continue;
                    }
                    Err(ImageError::Unsupported) => UnviewableReason::Unsupported,
                    Err(ImageError::TooLarge { .. }) => UnviewableReason::TooLarge,
                    Err(ImageError::Store(error)) => {
                        return Err(ChannelError::Kernel(error.into()));
                    }
                },
            };
            unviewable.push(Unviewable { what, reason });
        }
        Ok(prepared)
    }

    /// Retry transient failures; one that persists, or that is not retryable, leaves the content
    /// unavailable.
    ///
    /// Fatal and storage failures stop the connection.
    async fn read<T, F>(&self, fetch: impl Fn() -> F) -> Result<Fetched<T>, ChannelError>
    where
        F: Future<Output = Result<Fetched<T>, ChannelCallError>>,
    {
        let mut backoff = ChannelBackoff::default();
        let mut attempt = 1;
        loop {
            match super::super::health::call(&self.kernel, self.generation, fetch()).await {
                Err(ChannelError::Protocol(ChannelCallError::Protocol(fault))) if !fault.fatal => {
                    if !fault.failure.retryable || attempt == CHANNEL_ATTEMPTS {
                        return Ok(Fetched {
                            content: Err(fault.failure.message),
                            origin: fault.origin,
                        });
                    }
                    attempt += 1;
                    tokio::time::sleep(backoff.next(fault.retry_after)).await;
                }
                result => return result,
            }
        }
    }
}
