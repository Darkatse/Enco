//! Protocol samples shaped like the M33 SnowLuma probe, with envelopes from its event converter.
// The shapes of the login data, the signed message and reply IDs and the reply/at/text sequence
// were observed in M33; every account, group, name and ID is a placeholder.
// The surrounding event fields follow SnowLuma 8ffd314260e9e2b6967eeca494d87aa5be046c34,
// packages/onebot/src/event-converter/{envelope,to-message,element-codecs}.ts.
pub(super) const IDENTITY: &str =
    r#"{"status":"ok","retcode":0,"data":{"user_id":10001,"nickname":"Test Bot"}}"#;
pub(super) const PRIVATE_SENT: &str =
    r#"{"status":"ok","retcode":0,"data":{"message_id":-987654321}}"#;
pub(super) const GROUP_SENT: &str =
    r#"{"status":"ok","retcode":0,"data":{"message_id":-123456789}}"#;
pub(super) const GROUP_REPLY: &str = r#"{
    "time":1793220000,"self_id":10001,"post_type":"message",
    "message_type":"group","sub_type":"normal","message_id":-123456788,
    "group_id":20001,"user_id":10002,
    "sender":{"user_id":10002,"nickname":"Owner","card":"","role":"member","sex":"unknown","age":0},
    "message":[{"type":"reply","data":{"id":"-123456789"}},{"type":"at","data":{"qq":"10001"}},{"type":"text","data":{"text":" 这次再试试"}}],
    "raw_message":"[CQ:reply,id=-123456789][CQ:at,qq=10001] 这次再试试","font":0
}"#;
