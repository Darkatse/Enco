//! Files cross the mapper boundary as named locators, then verified SnowLuma byte streams.
use super::*;
use enco_sdk::{
    channel::{AttachmentKind, Content},
    types::UnviewableReason,
};

fn content(segments: Value, direct: bool) -> Result<Content, String> {
    let reading = received(frame(segments, direct))?;
    match reading.messages.into_iter().next().ok_or("message")?.action {
        Action::Message(content) => Ok(content),
        _ => Err("expected attachment message".into()),
    }
}

fn answer(data: Value) -> Response {
    response(&json!({"status":"ok", "retcode":0, "data":data}).to_string())
}

fn stream(frames: &[Value]) -> Response {
    response(
        &frames
            .iter()
            .map(|data| json!({"status":"ok", "retcode":0, "data":data}).to_string())
            .collect::<Vec<_>>()
            .join("\r\n\r\n"),
    )
}

fn chunks() -> Vec<Value> {
    vec![
        json!({"data_type":"file_info", "file_name":"report.pdf", "file_size":11}),
        json!({"data_type":"file_chunk", "index":0, "data":"aGVsbG8g"}),
        json!({"data_type":"file_chunk", "index":1, "data":"d29ybGQ="}),
        json!({"data_type":"file_complete", "total_chunks":2, "total_bytes":11}),
    ]
}

#[test]
fn media_keep_kind_names_and_use_uncredentialed_downloads() -> Result<(), String> {
    for (kind, name, filename) in [
        ("image", AttachmentKind::Image, "0123456789abcdef.jpg"),
        (
            "record",
            AttachmentKind::VoiceMessage,
            "0123456789abcdef.amr",
        ),
        ("video", AttachmentKind::Video, "0123456789abcdef.mp4"),
    ] {
        let url = "https://multimedia.nt.qq.com.cn/download?fileid=media";
        let content = content(
            json!([
                {"type":kind, "data":{"file":filename, "url":url}},
            ]),
            true,
        )?;
        assert_eq!(content.attachments.len(), 1);
        assert_eq!(content.attachments[0].name, name.name());
        assert!(content.unviewable.is_empty());
        let locator = &content.attachments[0].locator;
        assert_eq!(
            Plugin::fetch(locator.clone(), vec![], LIMIT),
            Ok(FetchStep::Download(url.into()))
        );
        assert_eq!(
            Plugin::fetch(locator.clone(), vec![response("media bytes")], LIMIT),
            Ok(FetchStep::File(b"media bytes".to_vec()))
        );
    }
    Ok(())
}

#[test]
fn missing_media_urls_are_unavailable_and_wrong_url_types_are_malformed() -> Result<(), String> {
    for kind in ["image", "record", "video"] {
        for data in [
            json!({"file":"opaque"}),
            json!({"file":"opaque", "url":""}),
            json!({"url":7}),
        ] {
            let content = content(json!([{"type":kind, "data":data}]), true)?;
            assert!(content.attachments.is_empty());
            assert_eq!(content.unviewable.len(), 1);
            assert!(
                matches!(&content.unviewable[0].reason, UnviewableReason::Unavailable(reason) if !reason.is_empty())
            );
        }
    }
    Ok(())
}

#[test]
fn private_and_group_files_resolve_ids_before_streaming_complete_bytes() -> Result<(), String> {
    for direct in [true, false] {
        let content = content(
            json!([{"type":"file", "data":{
                "name":"report.pdf", "file_id":"/opaque:id/with:separators", "file_hash":"hash:part/2", "file_size":11,
            }}]),
            direct,
        )?;
        assert_eq!(content.attachments[0].name, "report.pdf");
        assert!(content.unviewable.is_empty());
        let locator = &content.attachments[0].locator;
        let FetchStep::Request(resolve) =
            Plugin::fetch(locator.clone(), vec![], LIMIT).map_err(|e| e.failure.message)?
        else {
            return Err("file URL request".into());
        };
        assert_eq!(
            resolve.url,
            if direct {
                "/get_private_file_url"
            } else {
                "/get_group_file_url"
            }
        );
        let params: Value = serde_json::from_slice(resolve.body.as_deref().ok_or("params")?)
            .map_err(|e| e.to_string())?;
        assert_eq!(params["file_id"], "/opaque:id/with:separators");
        if direct {
            assert_eq!(params["file_hash"], "hash:part/2");
            assert!(params.get("group_id").is_none());
        } else {
            assert_eq!(params["group_id"], 20001);
            assert!(params.get("file_hash").is_none());
        }
        let resolved = answer(json!({"url":"http://43.129.1.2/asn.com/qqdownloadftnv5"}));
        let FetchStep::Request(download) =
            Plugin::fetch(locator.clone(), vec![resolved.clone()], LIMIT)
                .map_err(|e| e.failure.message)?
        else {
            return Err("file stream request".into());
        };
        assert_eq!(download.url, "/download_file_stream");
        let params: Value = serde_json::from_slice(download.body.as_deref().ok_or("params")?)
            .map_err(|e| e.to_string())?;
        assert_eq!(
            params,
            json!({"file":"http://43.129.1.2/asn.com/qqdownloadftnv5"})
        );
        assert_eq!(
            Plugin::fetch(locator.clone(), vec![resolved, stream(&chunks())], LIMIT),
            Ok(FetchStep::File(b"hello world".to_vec()))
        );
    }
    Ok(())
}

#[test]
fn file_names_and_context_determine_locators_and_local_failures() -> Result<(), String> {
    for (data, name, direct, available) in [
        (
            json!({"name":"", "file_id":"id", "file_hash":"hash"}),
            AttachmentKind::File.name(),
            true,
            true,
        ),
        (
            json!({"name":"report.pdf", "file_id":"id"}),
            "report.pdf",
            false,
            true,
        ),
        (
            json!({"name":"report.pdf", "file_id":"id"}),
            "report.pdf",
            true,
            false,
        ),
        (
            json!({"name":"report.pdf", "file_id":"id", "file_hash":""}),
            "report.pdf",
            true,
            false,
        ),
        (json!({"name":"broken.pdf"}), "broken.pdf", true, false),
        (
            json!({"name":"broken.pdf", "file_id":4}),
            "broken.pdf",
            true,
            false,
        ),
        (
            json!({"name":"broken.pdf", "file_id":""}),
            "broken.pdf",
            true,
            false,
        ),
    ] {
        let content = content(json!([{"type":"file", "data":data}]), direct)?;
        if available {
            assert_eq!(content.attachments[0].name, name);
            assert!(content.unviewable.is_empty());
        } else {
            assert!(content.attachments.is_empty());
            assert_eq!(content.unviewable[0].what, name);
            assert!(matches!(
                content.unviewable[0].reason,
                UnviewableReason::Unavailable(_)
            ));
        }
    }
    Ok(())
}

#[test]
fn quoted_files_use_the_quoted_groups_context() -> Result<(), String> {
    let quoted = answer(json!({
        "user_id":10002, "group_id":20001, "sender":{"nickname":"Owner"}, "time":1793275200,
        "message":[{"type":"file", "data":{"name":"report.pdf", "file_id":"group-file"}}],
    }));
    let FetchStep::Messages(messages) =
        Plugin::fetch("onebot:reply:-7".into(), vec![quoted], LIMIT)
            .map_err(|e| e.failure.message)?
    else {
        return Err("quoted messages".into());
    };
    assert_eq!(messages[0].attachments[0].name, "report.pdf");
    let FetchStep::Request(request) =
        Plugin::fetch(messages[0].attachments[0].locator.clone(), vec![], LIMIT)
            .map_err(|e| e.failure.message)?
    else {
        return Err("group file request".into());
    };
    assert_eq!(request.url, "/get_group_file_url");
    let params: Value = serde_json::from_slice(request.body.as_deref().ok_or("params")?)
        .map_err(|e| e.to_string())?;
    assert_eq!(params, json!({"group_id":20001, "file_id":"group-file"}));
    Ok(())
}

#[test]
fn streams_require_ordered_decodable_chunks_and_matching_completion() -> Result<(), String> {
    let content = content(
        json!([{"type":"file", "data":{"name":"report.pdf", "file_id":"id", "file_hash":"hash"}}]),
        true,
    )?;
    let locator = &content.attachments[0].locator;
    let mut cases = vec![response(""), response("not JSON"), stream(&chunks()[..3])];
    for (frame, field, value) in [
        (1, "index", json!(1)),
        (1, "data", json!("%%%")),
        (3, "total_chunks", json!(1)),
        (3, "total_bytes", json!(12)),
    ] {
        let mut frames = chunks();
        frames[frame][field] = value;
        cases.push(stream(&frames));
    }
    for streamed in cases {
        let result = Plugin::fetch(
            locator.clone(),
            vec![answer(json!({"url":"http://file"})), streamed],
            LIMIT,
        );
        assert!(result.is_err_and(|fault| fault.failure.code == "channel.bad_response"));
    }
    Ok(())
}

#[test]
fn file_lookup_and_stream_failures_preserve_connection_faults() -> Result<(), String> {
    let content = content(
        json!([{"type":"file", "data":{"name":"report.pdf", "file_id":"id", "file_hash":"hash"}}]),
        true,
    )?;
    let locator = &content.attachments[0].locator;
    for responses in [
        vec![answer(json!({"url":""}))],
        vec![response(
            r#"{"status":"failed","retcode":1200,"data":null,"wording":"file expired"}"#,
        )],
        vec![
            answer(json!({"url":"http://file"})),
            response(r#"{"status":"failed","retcode":1200,"data":null,"wording":"file expired"}"#),
        ],
        vec![answer(json!({"url":"http://file"})), status(404)],
    ] {
        assert!(matches!(
            Plugin::fetch(locator.clone(), responses, LIMIT),
            Ok(FetchStep::Unavailable(_))
        ));
    }
    assert!(Plugin::fetch(locator.clone(), vec![answer(json!({"url":4}))], LIMIT).is_err());
    for code in [401, 403, 429, 503] {
        for responses in [
            vec![status(code)],
            vec![answer(json!({"url":"http://file"})), status(code)],
        ] {
            let fault = Plugin::fetch(locator.clone(), responses, LIMIT)
                .err()
                .ok_or("connection fault")?;
            assert_eq!(fault.fatal, code == 401 || code == 403);
            assert_eq!(fault.failure.retryable, code == 429 || code == 503);
        }
    }
    Ok(())
}
