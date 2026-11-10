//! Reads an input's attachments and quoted messages through its mapper; earlier context fetches nothing.
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

    /// The message with these attachments in place of its protocol attachments.
    fn quoted<A>(self, message: ChannelQuotedMessage, attachments: Vec<A>) -> QuotedMessage<A> {
        let ChannelQuotedMessage {
            depth,
            sender,
            sender_name,
            conversation_name,
            sent_at,
            text,
            attachments: _,
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
            attachments,
            unviewable,
        }
    }
}

/// Earlier context keeps what the protocol carried, with attachments as descriptions.
pub(super) fn earlier(quote: ChannelQuote, identities: Identities<'_>) -> EarlierQuote {
    let ChannelQuote { kind, messages } = quote;
    let messages = match messages {
        ChannelQuoted::Fetch(_) => None,
        ChannelQuoted::Unavailable(reason) => Some(Quoted::Unavailable(reason)),
        ChannelQuoted::Messages(messages) => Some(Quoted::Messages(
            messages
                .into_iter()
                .map(|mut message| {
                    let attachments = std::mem::take(&mut message.attachments);
                    let attachments = attachments
                        .into_iter()
                        .map(|attachment| attachment.name)
                        .collect();
                    identities.quoted(message, attachments)
                })
                .collect(),
        )),
    };
    EarlierQuote { kind, messages }
}

impl Reader {
    /// Store the attachments of a message that may drive a Session, and of the messages it quotes.
    pub(super) async fn content(
        &self,
        content: ChannelContent<ChannelAttachment>,
        identities: Identities<'_>,
    ) -> Result<ChannelContent<Attachment, Quote>, ChannelError> {
        let ChannelContent {
            text,
            attachments,
            mut unviewable,
            quote,
        } = content;
        let quote = match quote {
            Some(quote) => Some(self.quote(quote, identities).await?),
            None => None,
        };
        Ok(ChannelContent {
            text,
            attachments: self.attachments(attachments, &mut unviewable).await?,
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
                    let attachments = std::mem::take(&mut message.attachments);
                    let attachments = self
                        .attachments(attachments, &mut message.unviewable)
                        .await?;
                    prepared.push(identities.quoted(message, attachments));
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

    async fn attachments(
        &self,
        attachments: Vec<ChannelAttachment>,
        unviewable: &mut Vec<Unviewable>,
    ) -> Result<Vec<Attachment>, ChannelError> {
        let mut prepared = Vec::with_capacity(attachments.len());
        for ChannelAttachment { name, locator } in attachments {
            let fetched = self.read(|| self.adapter.fetch_file(&locator)).await?;
            match fetched.content {
                Ok(bytes) => {
                    let stored = self
                        .kernel
                        .store_attachment(name, &bytes)
                        .await
                        .map_err(KernelError::from)?;
                    prepared.push(stored);
                }
                Err(reason) => unviewable.push(Unviewable {
                    what: name,
                    reason: UnviewableReason::Unavailable(reason),
                }),
            }
        }
        Ok(prepared)
    }

    /// Retry transient failures; one that persists, or that is not retryable, leaves the content
    /// unavailable.
    ///
    /// Fatal and storage failures stop the connection.
    pub(super) async fn read<T, F>(&self, fetch: impl Fn() -> F) -> Result<Fetched<T>, ChannelError>
    where
        F: Future<Output = Result<Fetched<T>, ChannelCallError>>,
    {
        let mut backoff = ChannelBackoff::default();
        let mut attempt = 1;
        loop {
            match super::super::health::call(&self.kernel, &self.code, fetch()).await {
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
