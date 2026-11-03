//! Live drafts and cards, epilogues and child topics translated to Bot API requests.
use crate::{
    channel::{Fault, Request, Response},
    limits::{SEND_TIMEOUT_MS, TOPIC_NAME_CHARS},
    wire::{answer, conversation, conversation_id, destination, request},
};
use enco_sdk::channel::{ActivityUpdate, ActivityView, CANCEL_LABEL, child_title};
use serde::Deserialize;
use serde_json::json;

/// The icon color given only to topics opened for child Sessions.
const CHILD_TOPIC_ICON_COLOR: i64 = 0xCB86DB;

/// Whether the chat with this ID is a private chat with a user: Telegram gives users positive IDs
/// and groups and channels negative ones.
///
/// Drafts and child topics exist only in private chats, so `presence` shows a draft exactly where
/// `activity` leaves the view where the replies go to it.
fn private(chat: &str) -> bool {
    !chat.starts_with('-')
}

pub(super) fn presence(id: String, view: ActivityView) -> Option<Request> {
    let (chat, thread) = conversation(&id);
    let mut body = destination(chat, thread);
    let method = if private(chat) {
        body["draft_id"] = json!(1);
        body["can_stop"] = json!(true);
        body["text"] = json!(view.text());
        "sendMessageDraft"
    } else {
        body["action"] = json!("typing");
        "sendChatAction"
    };
    Some(request(method, body, SEND_TIMEOUT_MS))
}

pub(super) fn activity(
    id: String,
    replies_here: bool,
    shown: Option<String>,
    update: ActivityUpdate,
) -> Option<Request> {
    let (chat, thread) = conversation(&id);
    let mut body = json!({"chat_id": chat});
    match update {
        ActivityUpdate::Remove => {
            body["message_id"] = json!(shown?);
            return Some(request("deleteMessage", body, SEND_TIMEOUT_MS));
        }
        ActivityUpdate::View(view) => {
            if shown.is_none() && replies_here && private(chat) {
                return None;
            }
            body["text"] = json!(view.text());
            let cancel =
                json!({"text": CANCEL_LABEL, "callback_data": "cancel", "style": "danger"});
            body["reply_markup"] = json!({"inline_keyboard": [[cancel]]});
        }
        ActivityUpdate::Epilogue(epilogue) => {
            body["text"] = json!(epilogue.text());
            if shown.is_some() {
                body["reply_markup"] = json!({"inline_keyboard": []});
            }
        }
    }
    let method = if let Some(message) = shown {
        body["message_id"] = json!(message);
        "editMessageText"
    } else {
        if let Some(thread) = thread {
            body["message_thread_id"] = json!(thread);
        }
        body["disable_notification"] = json!(true);
        "sendMessage"
    };
    Some(request(method, body, SEND_TIMEOUT_MS))
}

/// Child topics open only in private chats. Telegram refuses one when the owner has not turned on
/// topics for the bot, and `opened` gives that failure.
pub(super) fn open(beside: String, session: Vec<String>) -> Option<Request> {
    let (chat, _) = conversation(&beside);
    if !private(chat) {
        return None;
    }
    Some(request(
        "createForumTopic",
        json!({
            "chat_id": chat,
            "name": child_title(&session, TOPIC_NAME_CHARS),
            "icon_color": CHILD_TOPIC_ICON_COLOR,
        }),
        SEND_TIMEOUT_MS,
    ))
}

pub(super) fn opened(beside: String, response: Response) -> Result<String, Fault> {
    #[derive(Deserialize)]
    struct Topic {
        message_thread_id: i64,
    }
    let topic: Topic = answer("createForumTopic", response)?;
    let (chat, _) = conversation(&beside);
    Ok(conversation_id(chat, Some(topic.message_thread_id)))
}

/// A topic in a private chat cannot be closed, so putting it away deletes it.
pub(super) fn close(id: String) -> Option<Request> {
    let (chat, thread) = conversation(&id);
    let thread = thread?;
    Some(request(
        "deleteForumTopic",
        json!({"chat_id": chat, "message_thread_id": thread}),
        SEND_TIMEOUT_MS,
    ))
}
