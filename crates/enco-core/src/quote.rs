//! Quoted messages are input facts; their images and retrieval evidence stay with the input.
use crate::{DateTime, Image, MappingOrigin, Unviewable, Utc};
use serde::{Deserialize, Serialize};

/// Messages that an input replies to or forwards, with their stored images.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    /// How the input relates to these messages.
    pub kind: QuoteKind,
    /// The quoted content, or why it was not obtained.
    pub messages: Quoted<Image>,
    /// The fetch that produced this content; absent when it arrived with the input itself.
    pub fetched: Option<MappingOrigin>,
}

/// Messages that an earlier message quoted; images stay descriptions and nothing is fetched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EarlierQuote {
    /// How the earlier message relates to these messages.
    pub kind: QuoteKind,
    /// The content the protocol carried, or why it could not; absent when it needed a request.
    pub messages: Option<Quoted<String>>,
}

/// How a message relates to the messages it quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteKind {
    /// It answers the message, or the messages that message forwarded.
    Reply,
    /// It passes the messages on from elsewhere.
    Forward,
}

/// Quoted content, or why the protocol could not supply it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "snake_case")]
pub enum Quoted<A> {
    /// Messages in protocol order, with nested forwards immediately after their parent.
    Messages(Vec<QuotedMessage<A>>),
    /// Why the protocol could not supply the content.
    Unavailable(String),
}

/// One message inside a reply or forward, with the identity facts recorded at acceptance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotedMessage<A> {
    /// Nesting depth, starting at zero; a nested forward follows its parent one level deeper.
    pub depth: u8,
    /// The sender's protocol ID, when reported.
    pub sender: Option<String>,
    /// The sender's display name, when reported.
    pub sender_name: Option<String>,
    /// Whether the sender was the owner when the quoting message arrived.
    pub owner: bool,
    /// Whether this account sent the quoted message.
    pub yours: bool,
    /// The named original conversation, when it was elsewhere.
    pub conversation_name: Option<String>,
    /// When the message was first sent, when reported.
    pub sent_at: Option<DateTime<Utc>>,
    /// What the sender wrote, in the protocol's readable form.
    pub text: String,
    /// Stored images for an input, or image descriptions for an earlier message.
    pub images: Vec<A>,
    /// Content that could not be shown, and why.
    pub unviewable: Vec<Unviewable>,
}

impl Quote {
    /// The quoted images in the order their messages are listed.
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        let messages = match &self.messages {
            Quoted::Messages(messages) => messages.as_slice(),
            Quoted::Unavailable(_) => &[],
        };
        messages.iter().flat_map(|message| &message.images)
    }
}
