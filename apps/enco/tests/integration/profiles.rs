#![expect(clippy::unwrap_used, reason = "test fixtures fail with context")]
use crate::{protocol::Command, support::*};
use enco_core::{ContentHash, EventId, PlanItem, Role, SessionRecord};
use enco_kernel::Inspection;
use serde_json::json;
use wiremock::{Mock, MockServer, matchers::path};

async fn ask(daemon: &Daemon, session: &str) -> Inspection {
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: session.into(),
        })
        .await
        .unwrap();
    client
        .request(Command::Send {
            session: session.into(),
            text: "hello".into(),
            images: vec![],
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    finish(&mut client).await;
    serde_json::from_value(
        daemon
            .connect()
            .await
            .request(Command::Inspect {
                session: session.into(),
                attempt_id: None,
            })
            .await
            .unwrap(),
    )
    .unwrap()
}

fn system(inspection: &Inspection) -> String {
    inspection
        .request
        .messages
        .iter()
        .find(|message| message.role == Role::System)
        .unwrap()
        .joined_text()
}

#[tokio::test]
async fn profile_instructions_refresh_without_registration_and_keep_their_sources() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({"role": "assistant", "content": "done"}),
            "stop",
        ))
        .mount(&server)
        .await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let file = daemon.root.path().join("profiles/default.md");
    let sourced = |inspection: &Inspection, text: &[u8]| {
        inspection.plan.items.iter().any(|item| {
            matches!(item, PlanItem::Message { sources, .. }
                if sources.iter().any(|source| source.id == "instructions:profiles/default.md"
                    && source.hash == ContentHash::of(text)))
        })
    };
    std::fs::write(&file, "Default instructions, version one").unwrap();
    let first = ask(&daemon, "main").await;
    assert!(system(&first).contains("Default instructions, version one"));
    assert!(sourced(&first, b"Default instructions, version one"));
    std::fs::write(&file, "Default instructions, version two").unwrap();
    let changed = ask(&daemon, "main").await;
    assert!(system(&changed).contains("Default instructions, version two"));
    assert!(!system(&changed).contains("Default instructions, version one"));
    assert!(sourced(&changed, b"Default instructions, version two"));
    std::fs::remove_file(&file).unwrap();
    let removed = ask(&daemon, "main").await;
    assert!(!system(&removed).contains("Default instructions, version two"));
    assert!(system(&removed).contains(file.to_str().unwrap()));
    assert!(
        !removed
            .plan
            .items
            .iter()
            .any(|item| matches!(item, PlanItem::Message { sources, .. }
                if sources.iter().any(|source| source.id == "instructions:profiles/default.md")))
    );
    // Status reports this file as unmatched, so it is not read even where names ignore case.
    std::fs::write(file.with_file_name("Default.md"), "Other case").unwrap();
    assert!(!system(&ask(&daemon, "main").await).contains("Other case"));
    daemon.stop().await;
}

#[tokio::test]
async fn working_dir_cli_absolutizes_without_requiring_the_directory_to_exist() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({"role": "assistant", "content": "done"}),
            "stop",
        ))
        .mount(&server)
        .await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    ask(&daemon, "main").await;
    let home = daemon.root.path();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["working-dir", "main", "future-project"])
        .env("ENCO_HOME", home)
        .current_dir(home)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = std::fs::canonicalize(home).unwrap().join("future-project");
    assert!(!directory.exists());
    let sessions: Vec<SessionRecord> = serde_json::from_value(
        daemon
            .connect()
            .await
            .request(Command::Sessions { archived: false })
            .await
            .unwrap(),
    )
    .unwrap();
    let main = sessions
        .iter()
        .find(|session| session.name.as_str() == "main")
        .unwrap();
    assert_eq!(main.working_dir.path(), directory);
    let inspection = ask(&daemon, "main").await;
    assert!(system(&inspection).contains(directory.to_str().unwrap()));
    daemon.stop().await;
}
