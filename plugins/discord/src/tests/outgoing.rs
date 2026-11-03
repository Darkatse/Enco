use super::*;

#[test]
fn split_and_components_keep_long_tables_files_and_text_within_limits() -> Result<(), String> {
    let text = format!(
        "{}\n---\n| Header | Value |\n|---|---|\n| A | B |\n\n```rust\nlet code = 1;\n```\n",
        "😀".repeat(4100)
    );
    let file = FileData {
        name: "plot.png".into(),
        data: b"\x89PNG\r\n\x1a\nplot".into(),
    };
    let parts = Plugin::split(Outgoing {
        text: text.clone(),
        file: Some(file.clone()),
    });
    assert!(parts.len() >= 3);
    assert_eq!(
        parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>(),
        text
    );
    assert_eq!(parts.iter().filter(|part| part.file.is_some()).count(), 1);
    for part in &parts {
        assert!(part.text.encode_utf16().count() <= 4000);
    }
    let first =
        Plugin::send("thread".into(), parts[0].clone()).map_err(|error| error.failure.message)?;
    assert_eq!(body(&first)?["flags"], 32768);
    let last = Plugin::send("thread".into(), parts.last().ok_or("last part")?.clone())
        .map_err(|error| error.failure.message)?;
    let bytes = last.body.ok_or("multipart")?;
    let encoded = String::from_utf8_lossy(&bytes);
    assert!(encoded.contains("payload_json"));
    assert!(encoded.contains("attachment://plot.png"));
    assert!(encoded.contains(r#""type":12"#));
    assert!(encoded.contains(r#""type":14"#));
    assert!(encoded.contains("```\\n| Header"));
    assert!(
        bytes
            .windows(file.data.len())
            .any(|bytes| bytes == file.data)
    );
    Ok(())
}

#[test]
fn split_preserves_markdown_and_budgets_the_rendered_components() -> Result<(), String> {
    for text in [
        "| h | v |\n| - | - |\n| a | b |\n\n".repeat(200),
        format!("```rust\n{}\n```\n", "let value = 1;\n".repeat(400)),
        "paragraph\n---\n".repeat(200),
    ] {
        let parts = Plugin::split(Outgoing {
            text: text.clone(),
            file: None,
        });
        assert_eq!(
            parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        for part in parts {
            let request =
                Plugin::send("thread".into(), part).map_err(|error| error.failure.message)?;
            let payload = body(&request)?;
            let components = payload["components"][0]["components"]
                .as_array()
                .ok_or("components")?;
            assert!(components.len() < 40);
            let units: usize = components
                .iter()
                .filter_map(|component| component["content"].as_str())
                .map(|text| text.encode_utf16().count())
                .sum();
            assert!(units <= 4000);
        }
    }
    Ok(())
}

#[test]
fn runs_show_typing_and_a_card_created_edited_and_deleted_with_cancel() -> Result<(), String> {
    let activity = ActivityView {
        elapsed_seconds: 125,
        tools: (2..7)
            .map(|index| ToolActivity {
                name: format!("tool_{index}"),
                state: ToolState::Running,
                arguments: format!("*.rs {index}"),
            })
            .collect(),
    };
    let created = Plugin::activity(
        "thread".into(),
        true,
        None,
        ActivityUpdate::View(activity.clone()),
    )
    .ok_or("create")?;
    let payload = body(&created)?;
    assert_eq!(payload["flags"], 32768 | (1 << 12));
    let content = payload["components"][0]["components"][0]["content"]
        .as_str()
        .ok_or("text")?;
    assert!(content.starts_with("Working 2:05"));
    // Arguments may hold Markdown; the card shows them as written.
    assert!(content.contains(r"… tool\_2 · \*.rs 2"));
    assert!(content.contains(r"… tool\_6 · \*.rs 6"));
    assert_eq!(
        payload["components"][0]["components"][1]["components"][0]["custom_id"],
        "cancel"
    );
    let edited = Plugin::activity(
        "thread".into(),
        true,
        Some("card".into()),
        ActivityUpdate::View(activity),
    )
    .ok_or("edit")?;
    assert_eq!(edited.method, "PATCH");
    assert_eq!(body(&edited)?["flags"], 32768);
    let deleted = Plugin::activity(
        "thread".into(),
        true,
        Some("card".into()),
        ActivityUpdate::Remove,
    )
    .ok_or("delete")?;
    assert_eq!(deleted.method, "DELETE");
    assert_eq!(deleted.url, "/channels/thread/messages/card");
    let typing = Plugin::presence(
        "thread".into(),
        ActivityView {
            elapsed_seconds: 0,
            tools: vec![],
        },
    )
    .ok_or("typing")?;
    assert_eq!(
        (typing.method.as_str(), typing.url.as_str()),
        ("POST", "/channels/thread/typing")
    );
    Ok(())
}

#[test]
fn http_answers_preserve_evidence() -> Result<(), String> {
    assert_eq!(
        Plugin::answered(response(200, json!({"id":"sent"})))
            .map_err(|error| error.failure.message)?,
        Some("sent".into())
    );
    assert_eq!(
        Plugin::answered(response(200, json!([]))).map_err(|error| error.failure.message)?,
        None
    );
    assert!(
        matches!(Plugin::answered(response(429,json!({"retry_after":1.25}))),Err(error) if error.retry_after == Some(2) && !error.unknown && error.failure.retryable)
    );
    assert!(
        matches!(Plugin::answered(response(503,json!({}))),Err(error) if error.unknown && error.failure.retryable)
    );
    for malformed in [json!({}), json!({"id": 7}), json!(true)] {
        assert!(matches!(Plugin::answered(response(200, malformed)), Err(error) if error.unknown));
    }
    Ok(())
}
