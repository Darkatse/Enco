use enco_core::*;
use enco_host::native_tools;
use enco_kernel::{CallContext, Tool};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

async fn call(tool: &dyn Tool, args: serde_json::Value) -> Outcome {
    let serde_json::Value::Object(args) = args else {
        panic!("test arguments must be an object");
    };
    tool.call(
        CallContext {
            session: SessionId::new(),
            call: CallId::new(),
            cancel: CancellationToken::new(),
        },
        args,
    )
    .await
}

fn find(tools: &[Arc<dyn Tool>], name: &str) -> Arc<dyn Tool> {
    tools
        .iter()
        .find(|t| t.spec().name == name)
        .cloned()
        .unwrap_or_else(|| panic!("missing {name}"))
}

#[tokio::test]
async fn edits_require_unique_matches_and_reads_observe_line_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let tools = native_tools(dir.path().to_owned());
    let edit = find(&tools, "fs_edit");
    let read = find(&tools, "fs_read");
    let path = dir.path().join("note");
    std::fs::write(&path, "first\nsame\nsame\nlast\n").unwrap();
    let result = call(
        &*edit,
        serde_json::json!({ "path": "note", "old_string": "same", "new_string": "changed" }),
    )
    .await;
    assert!(matches!(result, Outcome::Failed { failure } if failure.code == code::TOOL_FAILED));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "first\nsame\nsame\nlast\n"
    );
    let replaced = call(
        &*edit,
        serde_json::json!({
            "path": "note",
            "old_string": "same",
            "new_string": "changed",
            "replace_all": true,
        }),
    )
    .await;
    assert!(matches!(replaced, Outcome::Ok { .. }));
    let Outcome::Ok { value } = call(
        &*read,
        serde_json::json!({ "path": "note", "offset": 3, "limit": 1 }),
    )
    .await
    else {
        panic!("read failed");
    };
    assert_eq!(value.as_str(), Some("[lines 3-3 of 4]\nchanged"));
}

#[tokio::test]
async fn shell_drains_both_pipes_preserves_exit_status_and_reports_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let shell = find(&native_tools(dir.path().to_owned()), "shell_exec");
    let Outcome::Ok { value } = call(
        &*shell,
        serde_json::json!({ "command": "printf out; printf err >&2; exit 7" }),
    )
    .await
    else {
        panic!("nonzero exit is a result, not a failed invocation");
    };
    assert_eq!(
        value,
        serde_json::json!({ "exit_code": 7, "stdout": "out", "stderr": "err" })
    );
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        call(
            &*shell,
            serde_json::json!({ "command": "exec sleep 60", "timeout_ms": 20 }),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Outcome::Unknown { failure } if failure.code == code::TIMEOUT));
}

#[tokio::test]
async fn background_command_returns_collected_output_after_shell_exit() {
    let dir = tempfile::tempdir().unwrap();
    let shell = find(&native_tools(dir.path().to_owned()), "shell_exec");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        call(
            &*shell,
            serde_json::json!({
                "command": "sleep 30 & echo $! > background.pid; echo started",
                "timeout_ms": 3000,
            }),
        ),
    )
    .await;

    // Stop the test's background process even if the bounded return assertion fails.
    let pid = std::fs::read_to_string(dir.path().join("background.pid")).unwrap();
    std::process::Command::new("kill")
        .args(["-TERM", pid.trim()])
        .status()
        .unwrap();
    let Outcome::Ok { value } = result.expect("shell waited for the inherited pipes") else {
        panic!("the shell exited successfully");
    };
    assert_eq!(value["exit_code"], 0);
    assert_eq!(value["stdout"], "started\n");
    assert!(value["note"].as_str().is_some_and(|note| !note.is_empty()));
}

#[tokio::test]
async fn replacing_an_existing_file_preserves_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let tools = native_tools(dir.path().to_owned());
    let path = dir.path().join("script.sh");
    std::fs::write(&path, "echo old\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

    for (name, args) in [
        (
            "fs_edit",
            serde_json::json!({"path": "script.sh", "old_string": "old", "new_string": "new"}),
        ),
        (
            "fs_write",
            serde_json::json!({"path": "script.sh", "content": "echo rewritten\n"}),
        ),
    ] {
        assert!(matches!(
            call(&*find(&tools, name), args).await,
            Outcome::Ok { .. }
        ));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}

#[tokio::test]
async fn writes_through_symlinks_preserve_the_link_and_update_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let tools = native_tools(dir.path().to_owned());
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    std::fs::write(&target, "old").unwrap();
    std::os::unix::fs::symlink("target.txt", &link).unwrap();
    let actual_path = std::fs::canonicalize(&target).unwrap();

    for (name, args, expected) in [
        (
            "fs_write",
            serde_json::json!({"path": "link.txt", "content": "new"}),
            "new",
        ),
        (
            "fs_edit",
            serde_json::json!({"path": "link.txt", "old_string": "new", "new_string": "edited"}),
            "edited",
        ),
    ] {
        let Outcome::Ok { value } = call(&*find(&tools, name), args).await else {
            panic!("write through symlink failed");
        };
        assert_eq!(value["path"], serde_json::json!(actual_path));
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            std::path::PathBuf::from("target.txt")
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), expected);
    }
}
