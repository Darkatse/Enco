//! Offline Bot API responses, shipped with the plugin so its tests run after scaffolding.

pub(super) const IDENTITY: &str = r#"{
  "ok": true,
  "result": {
    "id": 42,
    "is_bot": true,
    "first_name": "Helper",
    "username": "HelperBot"
  }
}
"#;

pub(super) const UPDATES: &str = r#"{
  "ok": true,
  "result": [
    {
      "update_id": 100,
      "message": {
        "message_id": 1,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "hello"
      }
    },
    {
      "update_id": 101,
      "message": {
        "message_id": 2,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "😀 @HelperBot",
        "entities": [
          {
            "type": "mention",
            "offset": 3,
            "length": 10
          }
        ]
      }
    },
    {
      "update_id": 102,
      "message": {
        "message_id": 3,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "/everyone@HelperBot on"
      }
    },
    {
      "update_id": 103,
      "message": {
        "message_id": 4,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "/cancel@OtherBot"
      }
    },
    {
      "update_id": 104,
      "message": {
        "message_id": 5,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "reply",
        "reply_to_message": {
          "message_id": 9,
          "date": 1793275200,
          "chat": {
            "id": -100,
            "type": "supergroup"
          },
          "from": {
            "id": 42,
            "is_bot": true,
            "first_name": "Helper",
            "username": "HelperBot"
          },
          "text": "Earlier answer"
        }
      }
    },
    {
      "update_id": 105,
      "message": {
        "message_id": 6,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "ask owner",
        "entities": [
          {
            "type": "text_mention",
            "offset": 4,
            "length": 5,
            "user": {
              "id": 42,
              "first_name": "Helper"
            }
          }
        ]
      }
    },
    {
      "update_id": 106,
      "message": {
        "message_id": 7,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "text": "picture",
        "photo": [
          {
            "file_id": "small",
            "width": 32,
            "height": 32
          },
          {
            "file_id": "large",
            "width": 640,
            "height": 480
          }
        ]
      }
    },
    {
      "update_id": 107,
      "message": {
        "message_id": 8,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "caption": "a file",
        "document": {
          "file_id": "doc",
          "file_name": "diagram.webp",
          "mime_type": "image/webp"
        }
      }
    },
    {
      "update_id": 108,
      "message": {
        "message_id": 9,
        "chat": {
          "id": -100,
          "type": "supergroup"
        },
        "from": {
          "id": 7,
          "first_name": "Ada",
          "last_name": "Lovelace"
        },
        "voice": {
          "file_id": "voice"
        }
      }
    }
  ]
}
"#;
