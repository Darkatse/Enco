//! Opt-in checks against real services; credentials stay in inherited environment variables.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../src/client.rs"]
mod client;

#[path = "../src/protocol.rs"]
mod protocol;
mod support;

use enco_core::*;
use protocol::Command;
use std::{process::Stdio, time::Duration};
use support::*;

async fn run(client: &mut client::Client, session: &str, text: &str) -> Vec<Entry> {
    client
        .send(Command::Send {
            session: session.into(),
            text: text.into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    let entries = finish_with_timeout(client, Duration::from_secs(150)).await;
    assert!(
        matches!(
            entries.last().unwrap().body,
            EntryBody::RunEnded {
                end: RunEnd::Completed,
                ..
            }
        ),
        "live Run did not complete"
    );
    entries
}

fn answer(entries: &[Entry]) -> String {
    entries
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::AttemptSettled {
                result: AttemptResult::Completed { message, .. },
                ..
            } => Some(message.joined_text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn graceful_stop(daemon: &mut Daemon) {
    let pid = daemon.child.id().unwrap().to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(30), daemon.child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
#[ignore = "calls DeepSeek and Gemini; set DEEPSEEK_KEY and GEMINI_KEY explicitly"]
async fn deepseek_and_gemini_complete_the_personal_assistant_scenario() {
    for name in ["DEEPSEEK_KEY", "GEMINI_KEY"] {
        assert!(
            std::env::var(name).is_ok(),
            "set {name} before running live validation"
        );
    }
    let mut daemon =
        Daemon::start_config(include_str!("../../../examples/deepseek-gemini.toml")).await;
    std::fs::write(
        daemon.root.path().join("workspace/AGENTS.md"),
        "This is an isolated validation workspace. Perform only the requested synthetic operations. \
         Never read credentials, environment variables, or files outside the workspace. \
         Use shell only for the requested printf command.",
    ).unwrap();
    let mut main = daemon.connect().await;
    main.request(Command::Subscribe {
        session: "live-main".into(),
    })
    .await
    .unwrap();
    println!("live: saving memory, writing a file and running a shell command");
    let first = run(
        &mut main,
        "live-main",
        "仅用于 P0 验收：请用 memory_save 原样保存非置顶记忆“P0验收偏好=tea-red”。\
         然后用 fs_write 写 live-note.txt，内容 p0-ok；\
         用 shell_exec 执行 printf 'p0-smoke\\n'。简短回复。",
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(daemon.root.path().join("workspace/live-note.txt"))
            .unwrap()
            .trim(),
        "p0-ok"
    );
    let shell_output = first.iter().find_map(|entry| {
        let EntryBody::ToolCallSettled {
            outcome: Outcome::Ok { value },
            ..
        } = &entry.body
        else {
            return None;
        };
        value.get("stdout").and_then(serde_json::Value::as_str)
    });
    assert_eq!(shell_output.map(str::trim), Some("p0-smoke"));
    let mut admin = daemon.connect().await;
    let state: enco_host::MemoryList =
        serde_json::from_value(admin.request(Command::Memories {}).await.unwrap()).unwrap();
    assert_eq!(state.memories.len(), 1);
    let id = state.memories[0].id;
    assert!(
        state.unindexed.is_empty(),
        "Gemini must have indexed the memory"
    );
    run(
        &mut main,
        "live-main",
        "请使用 memory_update 把刚才那条记忆原样更正为“P0验收偏好=tea-green”，\
         保留原 id，不要新建。简短回复。",
    )
    .await;
    let state: enco_host::MemoryList =
        serde_json::from_value(admin.request(Command::Memories {}).await.unwrap()).unwrap();
    assert_eq!(state.memories.len(), 1);
    assert_eq!(state.memories[0].id, id);
    assert!(state.memories[0].text.contains("tea-green"));
    assert!(!state.memories[0].text.contains("tea-red"));
    let mut other = daemon.connect().await;
    other
        .request(Command::Subscribe {
            session: "live-other".into(),
        })
        .await
        .unwrap();
    assert!(
        answer(
            &run(
                &mut other,
                "live-other",
                "换个话题：我保存的 P0 验收偏好是什么？简短回答。"
            )
            .await
        )
        .contains("tea-green")
    );
    drop(other);
    println!(
        "live: Gemini indexing, correction and cross-session recall passed; creating one-minute reminder"
    );
    run(
        &mut main,
        "live-main",
        "请使用 schedule_create 在一分钟后提醒我：检查 P0 验收结果。不要保存为长期记忆。简短回复。",
    )
    .await;
    let schedules: Vec<Schedule> =
        serde_json::from_value(admin.request(Command::Schedules {}).await.unwrap()).unwrap();
    assert_eq!(schedules.len(), 1);
    let reminder = schedules[0].clone();
    drop(main);
    drop(admin);
    graceful_stop(&mut daemon).await;
    assert!(!daemon.root.path().join("enco.sock").exists());
    daemon.restart().await;
    let mut main = daemon.connect().await;
    main.request(Command::Subscribe {
        session: "live-main".into(),
    })
    .await
    .unwrap();
    let mut after = daemon.connect().await;
    after
        .request(Command::Subscribe {
            session: "live-after".into(),
        })
        .await
        .unwrap();
    assert!(
        answer(
            &run(
                &mut after,
                "live-after",
                "重启后，我保存的 P0 验收偏好是什么？简短回答。"
            )
            .await
        )
        .contains("tea-green")
    );
    drop(after);
    println!("live: restart recall passed; waiting for the reminder");
    let mut admin = daemon.connect().await;
    let prior: Vec<Entry> = serde_json::from_value(
        admin
            .request(Command::Log {
                session: "live-main".into(),
                after: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    let reminder_pos = prior
        .iter()
        .position(|entry| is_reminder(entry, reminder.id));
    if !reminder_pos.is_some_and(|pos| {
        prior[pos..]
            .iter()
            .any(|e| matches!(e.body, EntryBody::RunEnded { .. }))
    }) {
        finish_with_timeout(&mut main, Duration::from_secs(150)).await;
    }
    let log: Vec<Entry> = serde_json::from_value(
        admin
            .request(Command::Log {
                session: "live-main".into(),
                after: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        log.iter()
            .filter(|entry| is_reminder(entry, reminder.id))
            .count(),
        1
    );
    assert!(
        admin
            .request(Command::Schedules {})
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut attempts = 0;
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut cached_input_tokens = 0;
    for name in ["live-main", "live-other", "live-after"] {
        let log: Vec<Entry> = serde_json::from_value(
            admin
                .request(Command::Log {
                    session: name.into(),
                    after: None,
                })
                .await
                .unwrap(),
        )
        .unwrap();
        for entry in log {
            match entry.body {
                EntryBody::AttemptStarted { plan, .. } => {
                    let hash = plan.to_string();
                    let plan: ContextPlan = serde_json::from_slice(
                        &std::fs::read(
                            daemon.root.path().join("blobs").join(&hash[..2]).join(hash),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    assert!(
                        !plan.omitted.iter().any(|o| o.source == "memory:semantic"),
                        "real embedding must succeed without fallback"
                    );
                    attempts += 1;
                }
                EntryBody::AttemptSettled {
                    result: AttemptResult::Completed { usage, .. },
                    ..
                } => {
                    input_tokens += usage.input_tokens;
                    output_tokens += usage.output_tokens;
                    cached_input_tokens += usage.cached_input_tokens.unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    graceful_stop(&mut daemon).await;
    println!(
        "live passed: {attempts} attempts; tokens input={}, output={}, cached_input={}; no embedding fallback",
        input_tokens, output_tokens, cached_input_tokens
    );
}
