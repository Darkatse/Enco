use super::{args::*, resolve_path};
use crate::limits::*;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

pub(super) struct Shell {
    pub workspace: PathBuf,
}

#[async_trait]
impl Tool for Shell {
    fn code(&self) -> CodeRef {
        super::code()
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell_exec".into(),
            description: format!(
                "Execute command with sh -c. cwd defaults to the workspace. \
                 timeout_ms defaults to {SHELL_DEFAULT_TIMEOUT_MS} and is at most {SHELL_MAX_TIMEOUT_MS}. \
                 Nonzero exit codes are returned as results. Cancellation stops the shell; \
                 detached descendants are not managed. After shell exit, output is collected \
                 for up to {} ms. Redirect background output to a file: cmd > out.log 2>&1 &.",
                SHELL_EXIT_GRACE.as_millis()
            ),
            effect: Effect::SideEffect,
            input_schema: closed_object_schema(
                json!({
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 1, "maximum": SHELL_MAX_TIMEOUT_MS },
                }),
                &["command"],
            ),
        }
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.execute(ctx, &args).await {
            Ok(outcome) => outcome,
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl Shell {
    async fn execute(
        &self,
        ctx: CallContext,
        args: &Map<String, Value>,
    ) -> Result<Outcome, Failure> {
        let command = string(args, "command")?;
        let timeout_ms = integer(
            args,
            "timeout_ms",
            SHELL_DEFAULT_TIMEOUT_MS,
            1,
            SHELL_MAX_TIMEOUT_MS,
        )?;
        let cwd = match args.get("cwd") {
            None => self.workspace.clone(),
            Some(_) => resolve_path(&self.workspace, string(args, "cwd")?),
        };
        let child = Command::new("sh")
            .args(["-c", command])
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| failure(code::TOOL_FAILED, format!("{}: {e}", cwd.display())))?;
        collect_output(child, timeout_ms, &ctx.cancel).await
    }
}

async fn collect_output(
    mut child: Child,
    timeout_ms: u64,
    cancel: &CancellationToken,
) -> Result<Outcome, Failure> {
    let Some(mut stdout) = child.stdout.take() else {
        return Err(failure(code::TOOL_FAILED, "shell stdout was not piped"));
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err(failure(code::TOOL_FAILED, "shell stderr was not piped"));
    };
    let mut captured_stdout = CapturedOutput::default();
    let mut captured_stderr = CapturedOutput::default();
    let (mut stdout_open, mut stderr_open) = (true, true);
    let (mut stdout_buffer, mut stderr_buffer) = ([0; 8192], [0; 8192]);
    let deadline = tokio::time::sleep(Duration::from_millis(timeout_ms));
    let exit_grace = tokio::time::sleep(SHELL_EXIT_GRACE);
    tokio::pin!(deadline, exit_grace);
    let mut status: Option<std::process::ExitStatus> = None;
    // Descendants can inherit the pipes. Once the shell exits, bound the remaining drain.
    let interruption = loop {
        if let Some(status) = status
            && ((!stdout_open && !stderr_open) || exit_grace.is_elapsed())
        {
            let mut value = json!({
                "exit_code": status.code(),
                "stdout": captured_stdout.text(),
                "stderr": captured_stderr.text(),
            });
            if stdout_open || stderr_open {
                value["note"] = json!(
                    "Shell exited; background processes may still be running. \
                     Redirect their output to a file (cmd > out.log 2>&1 &)."
                );
            }
            return Ok(Outcome::Ok { value });
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break failure(
                code::CANCELLED,
                "command was cancelled and killed; it may have partially run",
            ),
            _ = &mut deadline => break failure(
                code::TIMEOUT,
                format!("command timed out after {timeout_ms} ms and was killed; it may have partially run"),
            ),
            result = child.wait(), if status.is_none() => match result {
                Ok(exit) => {
                    status = Some(exit);
                    exit_grace.as_mut().reset(tokio::time::Instant::now() + SHELL_EXIT_GRACE);
                }
                Err(e) => break failure(code::TOOL_FAILED, e.to_string()),
            },
            _ = &mut exit_grace, if status.is_some() => {},
            read = stdout.read(&mut stdout_buffer), if stdout_open => match read {
                Ok(0) => stdout_open = false,
                Ok(n) => captured_stdout.push(&stdout_buffer[..n]),
                Err(e) => break failure(code::TOOL_FAILED, e.to_string()),
            },
            read = stderr.read(&mut stderr_buffer), if stderr_open => match read {
                Ok(0) => stderr_open = false,
                Ok(n) => captured_stderr.push(&stderr_buffer[..n]),
                Err(e) => break failure(code::TOOL_FAILED, e.to_string()),
            },
        }
    };
    // P0 owns the shell process, not detached descendants. Close pipes after reaping it.
    if let Err(error) = child.kill().await {
        tracing::debug!(%error, "shell kill raced with exit");
    }
    if let Err(error) = child.wait().await {
        return Ok(Outcome::Unknown {
            failure: failure(code::TOOL_FAILED, format!("cannot reap shell: {error}")),
        });
    }
    Ok(Outcome::Unknown {
        failure: interruption,
    })
}

#[derive(Default)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

impl CapturedOutput {
    fn push(&mut self, bytes: &[u8]) {
        let remaining = SHELL_OUTPUT_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        self.truncated |= bytes.len() > remaining;
    }

    fn text(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.bytes).into_owned();
        if self.truncated {
            text.push_str("\n[truncated]");
        }
        text
    }
}
