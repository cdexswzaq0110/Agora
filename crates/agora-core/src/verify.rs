//! 驗證流程 — run the project's own check (tests, build, lint) and keep the evidence.
//!
//! A relay session is only graded `Confirmed` when this command ran and passed; otherwise
//! the reviewer's approval is an inference, not an observation.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::model::{now_ms, VerifyRun};
use crate::provider::proc::{kill_tree, no_window, tail, PAID_API_ENV};

pub const OUTPUT_TAIL_CHARS: usize = 6000;

fn shell(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        // chcp 65001: keep tool output UTF-8 so Chinese text survives (see Agent Kernel
        // docs/lessons/0002-windows-console-encoding.md).
        c.arg("/D").arg("/S").arg("/C");
        c.raw_arg(format!("\"chcp 65001 >nul & {command}\""));
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    }
}

pub async fn run_verify(command: &str, cwd: &Path, round: u32, timeout: Duration, cancel: &CancellationToken) -> VerifyRun {
    let started_at = now_ms();
    let t0 = Instant::now();
    let mut run = VerifyRun {
        command: command.to_string(),
        round,
        exit_code: None,
        ok: false,
        timed_out: false,
        output_tail: String::new(),
        started_at,
        duration_ms: 0,
    };
    let mut cmd = shell(command);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .kill_on_drop(true);
    for v in PAID_API_ENV {
        cmd.env_remove(v);
    }
    no_window(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            run.output_tail = format!("無法執行驗證指令：{e}");
            run.duration_ms = t0.elapsed().as_millis() as u64;
            return run;
        }
    };
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let out_task = tokio::spawn(async move {
        let mut b = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = s.read_to_end(&mut b).await;
        }
        b
    });
    let err_task = tokio::spawn(async move {
        let mut b = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = s.read_to_end(&mut b).await;
        }
        b
    });

    tokio::select! {
        status = child.wait() => {
            if let Ok(s) = status {
                run.exit_code = s.code();
                run.ok = s.success();
            }
        }
        _ = tokio::time::sleep(timeout) => {
            run.timed_out = true;
            kill_tree(&mut child).await;
        }
        _ = cancel.cancelled() => {
            kill_tree(&mut child).await;
            run.output_tail = "已取消".into();
        }
    }
    let stdout = out_task.await.unwrap_or_default();
    let stderr = err_task.await.unwrap_or_default();
    let mut combined = String::from_utf8_lossy(&stdout).to_string();
    let stderr = String::from_utf8_lossy(&stderr);
    if !stderr.trim().is_empty() {
        combined.push_str("\n[stderr]\n");
        combined.push_str(&stderr);
    }
    if run.output_tail.is_empty() {
        run.output_tail = tail(combined.trim(), OUTPUT_TAIL_CHARS);
    }
    run.duration_ms = t0.elapsed().as_millis() as u64;
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_success_and_failure() {
        let dir = std::env::temp_dir();
        let tok = CancellationToken::new();
        let ok = run_verify("echo agora-ok", &dir, 1, Duration::from_secs(30), &tok).await;
        assert!(ok.ok, "{ok:?}");
        assert!(ok.output_tail.contains("agora-ok"));
        let bad = run_verify("exit 3", &dir, 1, Duration::from_secs(30), &tok).await;
        assert!(!bad.ok);
        assert_eq!(bad.exit_code, Some(3));
    }

    #[tokio::test]
    async fn quoted_arguments_survive_the_shell() {
        let dir = std::env::temp_dir();
        let tok = CancellationToken::new();
        let r = run_verify("echo \"a b\" && echo c", &dir, 1, Duration::from_secs(30), &tok).await;
        assert!(r.ok, "{r:?}");
        assert!(r.output_tail.contains('c'));
    }
}
