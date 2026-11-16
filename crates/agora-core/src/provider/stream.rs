//! Spawn a CLI, feed the prompt on stdin, and hand each stdout line to a parser — with
//! cancellation, a wall-clock timeout, and a captured stderr tail.

use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::proc::{kill_tree, tail};

pub enum LineAction {
    Continue,
    /// Stop immediately and kill the process (used by billing guards).
    Abort(String),
}

#[derive(Debug, Default)]
pub struct RunResult {
    pub exit_code: Option<i32>,
    pub stderr_tail: String,
    pub cancelled: bool,
    pub timed_out: bool,
    pub aborted: Option<String>,
    pub spawn_error: Option<String>,
}

pub async fn run_streaming(
    mut cmd: Command,
    stdin_text: String,
    timeout: Duration,
    cancel: CancellationToken,
    mut on_line: impl FnMut(&str) -> LineAction,
) -> RunResult {
    let mut res = RunResult::default();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            res.spawn_error = Some(e.to_string());
            return res;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            let _ = stdin.write_all(stdin_text.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let stderr_task = child.stderr.take().map(|mut err| {
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf).await;
            tail(&String::from_utf8_lossy(&buf), 4000)
        })
    });

    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        loop {
            buf.clear();
            tokio::select! {
                n = reader.read_until(b'\n', &mut buf) => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            // Lossy: a stray non-UTF-8 byte must not end the stream.
                            let line = String::from_utf8_lossy(&buf);
                            let line = line.trim_end_matches(['\r', '\n']);
                            if line.is_empty() {
                                continue;
                            }
                            if let LineAction::Abort(msg) = on_line(line) {
                                res.aborted = Some(msg);
                                kill_tree(&mut child).await;
                                break;
                            }
                        }
                    }
                }
                _ = cancel.cancelled() => {
                    res.cancelled = true;
                    kill_tree(&mut child).await;
                    break;
                }
                _ = &mut deadline => {
                    res.timed_out = true;
                    kill_tree(&mut child).await;
                    break;
                }
            }
        }
    }

    match tokio::time::timeout(Duration::from_secs(15), child.wait()).await {
        Ok(Ok(status)) => res.exit_code = status.code(),
        _ => kill_tree(&mut child).await,
    }
    if let Some(t) = stderr_task {
        if let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(5), t).await {
            res.stderr_tail = s;
        }
    }
    res
}

/// Run a short command to completion and return trimmed stdout (for `--version` etc.).
pub async fn run_capture(mut cmd: Command, timeout: Duration) -> Result<String, String> {
    cmd.stdin(std::process::Stdio::null());
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| "逾時".to_string())?
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { stdout } else { err })
    }
}
