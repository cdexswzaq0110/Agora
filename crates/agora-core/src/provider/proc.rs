//! Process helpers shared by the CLI providers.

use std::path::Path;
use std::process::Stdio;
use tokio::process::{Child, Command};

/// Environment variables that would make a CLI bill an API key (or route to a paid
/// third-party endpoint) instead of the user's subscription. Always removed before spawning.
pub const PAID_API_ENV: &[&str] = &[
    // Claude Code
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "AWS_BEARER_TOKEN_BEDROCK",
    // Codex
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "OPENAI_BASE_URL",
    "AZURE_OPENAI_API_KEY",
];

/// Variables set when Agora itself runs inside a Claude Code session; a nested CLI should not
/// inherit them.
const NESTING_ENV: &[&str] = &["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"];

/// A `Command` with piped stdio, no console window on Windows, killed on drop, and every
/// paid-API variable scrubbed.
pub fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for v in PAID_API_ENV.iter().chain(NESTING_ENV) {
        cmd.env_remove(v);
    }
    no_window(&mut cmd);
    cmd
}

/// Without this, every child spawned from the GUI (windows-subsystem) desktop app flashes a
/// console window.
pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Kill the child and, on Windows, its whole process tree (CLI agents spawn tool processes).
pub async fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let mut k = Command::new("taskkill");
        k.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        no_window(&mut k);
        let _ = k.status().await;
    }
    let _ = child.kill().await;
}

/// Last `max` characters of `s` (char-boundary safe).
pub fn tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        let t: String = s.chars().skip(n - max).collect();
        format!("…{t}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_scrubs_every_paid_api_variable() {
        let cmd = command(Path::new("does-not-matter"));
        let removed: Vec<String> = cmd
            .as_std()
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().to_string())
            .collect();
        for v in PAID_API_ENV {
            assert!(removed.iter().any(|r| r.eq_ignore_ascii_case(v)), "{v} not scrubbed");
        }
    }

    #[test]
    fn tail_is_char_safe() {
        assert_eq!(tail("abc", 5), "abc");
        assert_eq!(tail("一二三四", 2), "…三四");
    }
}
