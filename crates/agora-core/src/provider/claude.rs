//! Claude Code CLI provider: `claude -p --output-format stream-json --verbose`.
//!
//! Billing guard: the `system/init` line reports `apiKeySource`. On a Claude subscription
//! (OAuth) it is `"none"`; anything else means an API key would be billed, so the run is
//! killed before the first model call completes.
//!
//! Gotcha (docs/lessons/0001): an auth failure arrives as `{"type":"result",
//! "subtype":"success","is_error":true,"result":"Failed to authenticate…"}` — `subtype` alone
//! would turn the error message into the model's "answer". Always check `is_error`.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

use super::proc;
use super::resolve::resolve_claude;
use super::stream::{run_capture, run_streaming, LineAction, RunResult};
use super::{Health, ModelOption, Provider, ProviderEvent, Sink, TurnOutput, TurnRequest};
use crate::model::{now_ms, Access, Family, TokenUsage, ToolNote};
use crate::quota::from_claude_rate_limit_info;

pub const LOGIN_FIX: &str = "在終端機執行：claude auth login（選 Claude 訂閱帳號登入，不要用 API key／Console）";

pub struct ClaudeCli {
    id: String,
    exe_override: Option<String>,
}

impl ClaudeCli {
    pub fn new(exe_override: Option<String>) -> Self {
        ClaudeCli { id: "claude".into(), exe_override }
    }

    fn exe(&self) -> Option<PathBuf> {
        resolve_claude(self.exe_override.as_deref())
    }
}

pub fn build_args(model: Option<&str>, access: Access, clean_config: bool) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--no-session-persistence",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
        a.push("--model".into());
        a.push(m.trim().into());
    }
    if clean_config {
        // No user-level MCP servers (e.g. desktop-control tools), plugins or hooks.
        a.extend(["--strict-mcp-config", "--setting-sources", "project,local"].map(String::from));
    }
    match access {
        // dontAsk: anything not pre-approved is denied (no prompt can be answered in -p mode).
        Access::ReadOnly => {
            a.extend(["--permission-mode", "dontAsk", "--allowedTools", "Read,Grep,Glob,WebSearch,WebFetch"].map(String::from));
        }
        Access::Write => {
            a.extend(["--permission-mode", "acceptEdits"].map(String::from));
        }
    }
    a
}

fn summarize_tool_input(input: &Value) -> String {
    for k in ["file_path", "path", "command", "pattern", "url", "query", "description", "prompt"] {
        if let Some(s) = input.get(k).and_then(Value::as_str) {
            return crate::compact::truncate_chars(s, 120);
        }
    }
    String::new()
}

fn looks_like_auth_error(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    ["authenticate", "oauth", "not logged in", "login", "invalid api key", "unauthorized"].iter().any(|k| l.contains(k))
}

/// Pure line parser — unit-tested against recorded shapes.
#[derive(Default)]
pub struct ClaudeStreamParser {
    provider_id: String,
    pub streamed: String,
    pub final_text: Option<String>,
    pub error: Option<String>,
    pub usage: Option<TokenUsage>,
    pub saw_result: bool,
    pub api_key_source: Option<String>,
    pub unsupported_model: bool,
    pub auth_error: bool,
}

impl ClaudeStreamParser {
    pub fn new(provider_id: &str) -> Self {
        ClaudeStreamParser { provider_id: provider_id.into(), ..Default::default() }
    }

    /// `Err(reason)` aborts the run (billing guard).
    pub fn feed(&mut self, line: &str, now: u64) -> Result<Vec<ProviderEvent>, String> {
        let Ok(o) = serde_json::from_str::<Value>(line) else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        match o.get("type").and_then(Value::as_str) {
            Some("system") if o.get("subtype").and_then(Value::as_str) == Some("init") => {
                let src = o.get("apiKeySource").and_then(Value::as_str).unwrap_or("none").to_string();
                self.api_key_source = Some(src.clone());
                if src != "none" {
                    return Err(format!(
                        "計費保護：Claude CLI 回報使用 API key（apiKeySource = {src}）。Agora 只使用訂閱額度，已在呼叫模型前中止。請移除 API key 設定並用 `claude auth login` 以訂閱帳號登入。"
                    ));
                }
            }
            Some("stream_event") => {
                let ev = o.get("event").unwrap_or(&Value::Null);
                match ev.get("type").and_then(Value::as_str) {
                    Some("message_start") if !self.streamed.is_empty() => {
                        self.streamed.push_str("\n\n");
                        out.push(ProviderEvent::Delta("\n\n".into()));
                    }
                    Some("content_block_delta") => {
                        let d = ev.get("delta").unwrap_or(&Value::Null);
                        if d.get("type").and_then(Value::as_str) == Some("text_delta") {
                            if let Some(t) = d.get("text").and_then(Value::as_str) {
                                self.streamed.push_str(t);
                                out.push(ProviderEvent::Delta(t.to_string()));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") => {
                if let Some(blocks) = o.pointer("/message/content").and_then(Value::as_array) {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                            out.push(ProviderEvent::Tool(ToolNote {
                                name: b.get("name").and_then(Value::as_str).unwrap_or("tool").to_string(),
                                detail: summarize_tool_input(b.get("input").unwrap_or(&Value::Null)),
                            }));
                        }
                    }
                }
            }
            Some("rate_limit_event") => {
                if let Some(info) = o.get("rate_limit_info") {
                    if let Some(s) = from_claude_rate_limit_info(info, &self.provider_id, now) {
                        out.push(ProviderEvent::Quota(s));
                    }
                }
            }
            Some("result") => {
                self.saw_result = true;
                if let Some(u) = o.get("usage") {
                    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    self.usage = Some(TokenUsage {
                        input_tokens: g("input_tokens") + g("cache_creation_input_tokens") + g("cache_read_input_tokens"),
                        output_tokens: g("output_tokens"),
                        cached_input_tokens: g("cache_read_input_tokens"),
                    });
                }
                let text = o.get("result").and_then(Value::as_str).unwrap_or("").to_string();
                let is_error = o.get("is_error").and_then(Value::as_bool).unwrap_or(false)
                    || o.get("subtype").and_then(Value::as_str).is_some_and(|s| s.starts_with("error"));
                if is_error {
                    let msg = if text.is_empty() {
                        o.get("errors")
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| format!("Claude CLI 回報錯誤（{}）", o.get("subtype").and_then(Value::as_str).unwrap_or("?")))
                    } else {
                        text
                    };
                    let lower = msg.to_ascii_lowercase();
                    if looks_like_auth_error(&msg) {
                        self.auth_error = true;
                        self.error = Some(format!("Claude CLI 未登入或登入已過期：{msg}。{LOGIN_FIX}"));
                    } else if lower.contains("model") && (lower.contains("not found") || lower.contains("invalid") || lower.contains("not available")) {
                        self.unsupported_model = true;
                        self.error = Some(format!("此模型不適用於你的 Claude 方案：{msg}"));
                    } else {
                        self.error = Some(msg);
                    }
                } else {
                    self.final_text = Some(text);
                }
            }
            _ => {}
        }
        Ok(out)
    }

    pub fn finish(self, run: &RunResult) -> TurnOutput {
        let mut out = TurnOutput { usage: self.usage.clone(), unsupported_model: self.unsupported_model, ..Default::default() };
        if let Some(a) = &run.aborted {
            out.error = Some(a.clone());
            out.guard_tripped = true;
            out.text = self.streamed;
            return out;
        }
        if let Some(e) = &run.spawn_error {
            out.error = Some(format!("無法啟動 claude：{e}"));
            return out;
        }
        if run.cancelled {
            out.error = Some("已取消".into());
            out.text = self.streamed;
            return out;
        }
        if run.timed_out {
            out.error = Some("逾時，已終止 claude 程序".into());
            out.text = self.streamed;
            return out;
        }
        if let Some(e) = self.error {
            out.error = Some(e);
            out.text = self.streamed;
            return out;
        }
        match self.final_text {
            Some(t) if self.saw_result => {
                out.ok = true;
                out.text = if t.trim().is_empty() { self.streamed } else { t };
            }
            _ => {
                out.error = Some(format!(
                    "claude 沒有回傳結果（exit code {}）{}",
                    run.exit_code.map_or("?".into(), |c| c.to_string()),
                    if run.stderr_tail.trim().is_empty() { String::new() } else { format!("：{}", run.stderr_tail.trim()) }
                ));
                out.text = self.streamed;
            }
        }
        out
    }
}

#[async_trait]
impl Provider for ClaudeCli {
    fn id(&self) -> &str {
        &self.id
    }
    fn family(&self) -> Family {
        Family::Claude
    }
    fn display_name(&self) -> String {
        "Claude（Claude Code CLI）".into()
    }
    fn models(&self) -> Vec<ModelOption> {
        [("", "CLI 預設"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")]
            .iter()
            .map(|(id, label)| ModelOption { id: id.to_string(), label: label.to_string() })
            .collect()
    }

    async fn health(&self) -> Health {
        let checked_at = now_ms();
        let Some(exe) = self.exe() else {
            return Health {
                ok: false,
                summary: "找不到 Claude Code CLI".into(),
                fix: Some("安裝：npm install -g @anthropic-ai/claude-code，或在設定中指定 claude.exe 路徑".into()),
                version: None,
                account: None,
                executable: None,
                checked_at,
            };
        };
        let mut v = proc::command(&exe);
        v.arg("--version");
        let version = run_capture(v, Duration::from_secs(20)).await.ok().map(|s| s.lines().next().unwrap_or("").to_string());
        let mut a = proc::command(&exe);
        a.args(["auth", "status", "--json"]);
        let status = run_capture(a, Duration::from_secs(25)).await;
        let mut h = Health {
            ok: false,
            summary: String::new(),
            fix: None,
            version,
            account: None,
            executable: Some(exe.display().to_string()),
            checked_at,
        };
        let parsed = status
            .as_deref()
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            // `auth status` exits non-zero when logged out but still prints JSON.
            .or_else(|| status.as_ref().err().and_then(|s| serde_json::from_str::<Value>(s).ok()));
        match parsed {
            None => {
                h.summary = "無法讀取 Claude 登入狀態".into();
                h.fix = Some(format!("{LOGIN_FIX}；若仍失敗請執行 claude doctor"));
            }
            Some(j) => {
                let logged_in = j.get("loggedIn").and_then(Value::as_bool).unwrap_or(false);
                let method = j.get("authMethod").and_then(Value::as_str).unwrap_or("unknown").to_string();
                let api = j.get("apiProvider").and_then(Value::as_str).unwrap_or("firstParty").to_string();
                let ml = method.to_ascii_lowercase();
                if !logged_in {
                    h.summary = "Claude CLI 尚未登入或登入已過期".into();
                    h.fix = Some(LOGIN_FIX.into());
                } else if api != "firstParty" {
                    h.summary = format!("Claude CLI 設定為 {api}（雲端供應商計費），Agora 不會使用");
                    h.fix = Some("移除 CLAUDE_CODE_USE_BEDROCK／VERTEX 等設定後重新登入".into());
                } else if ml.contains("api") || ml.contains("console") {
                    h.summary = format!("Claude CLI 以 API key 方式登入（{method}），Agora 不會使用");
                    h.fix = Some(LOGIN_FIX.into());
                } else {
                    h.ok = true;
                    h.summary = "已登入（訂閱）".into();
                    h.account = Some(method);
                }
            }
        }
        h
    }

    async fn run(&self, req: TurnRequest, sink: Sink) -> TurnOutput {
        let Some(exe) = self.exe() else {
            return TurnOutput::fail("找不到 Claude Code CLI（npm install -g @anthropic-ai/claude-code）");
        };
        let mut cmd = proc::command(&exe);
        cmd.args(build_args(req.model.as_deref(), req.access, req.clean_config)).current_dir(&req.cwd);
        let mut parser = ClaudeStreamParser::new(&self.id);
        let res = run_streaming(cmd, req.prompt, req.timeout, req.cancel, |line| match parser.feed(line, now_ms()) {
            Ok(events) => {
                for e in events {
                    let _ = sink.send(e);
                }
                LineAction::Continue
            }
            Err(reason) => LineAction::Abort(reason),
        })
        .await;
        parser.finish(&res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(lines: &[&str]) -> (ClaudeStreamParser, Vec<ProviderEvent>, Option<String>) {
        let mut p = ClaudeStreamParser::new("claude");
        let mut evs = Vec::new();
        for l in lines {
            match p.feed(l, 1) {
                Ok(e) => evs.extend(e),
                Err(reason) => return (p, evs, Some(reason)),
            }
        }
        (p, evs, None)
    }

    #[test]
    fn streams_text_tools_quota_and_final_result() {
        let lines = [
            r#"{"type":"system","subtype":"init","apiKeySource":"none","model":"claude-haiku"}"#,
            r#"{"type":"stream_event","event":{"type":"message_start"}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"你好"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs"}}]}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":{"five_hour":{"utilization":0.3,"resetsAt":10}}}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"最終答案","usage":{"input_tokens":10,"cache_read_input_tokens":5,"output_tokens":7}}"#,
        ];
        let (p, evs, abort) = feed_all(&lines);
        assert!(abort.is_none());
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::Delta(t) if t == "你好")));
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::Tool(n) if n.name == "Read" && n.detail == "src/main.rs")));
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::Quota(_))));
        let out = p.finish(&RunResult::default());
        assert!(out.ok);
        assert_eq!(out.text, "最終答案");
        assert_eq!(out.usage.unwrap().input_tokens, 15);
    }

    #[test]
    fn api_key_billing_source_aborts_before_any_answer() {
        let (p, _, abort) = feed_all(&[r#"{"type":"system","subtype":"init","apiKeySource":"ANTHROPIC_API_KEY"}"#]);
        let reason = abort.expect("must abort");
        assert!(reason.contains("計費保護"));
        let out = p.finish(&RunResult { aborted: Some(reason), ..Default::default() });
        assert!(!out.ok && out.guard_tripped);
    }

    #[test]
    fn is_error_with_success_subtype_is_an_error_not_an_answer() {
        // Shape recorded from claude 2.1.252 with an expired OAuth session.
        let (p, _, _) = feed_all(&[
            r#"{"type":"system","subtype":"init","apiKeySource":"none"}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"result":"Failed to authenticate: OAuth session expired and could not be refreshed"}"#,
        ]);
        assert!(p.auth_error);
        let out = p.finish(&RunResult::default());
        assert!(!out.ok);
        assert!(out.error.unwrap().contains("claude auth login"));
    }

    #[test]
    fn missing_result_reports_exit_code_and_stderr() {
        let p = ClaudeStreamParser::new("claude");
        let out = p.finish(&RunResult { exit_code: Some(1), stderr_tail: "boom".into(), ..Default::default() });
        assert!(!out.ok);
        assert!(out.error.unwrap().contains("boom"));
    }

    #[test]
    fn read_only_turns_cannot_edit() {
        let a = build_args(Some("sonnet"), Access::ReadOnly, true).join(" ");
        assert!(a.contains("--permission-mode dontAsk"));
        assert!(a.contains("--allowedTools Read,Grep,Glob"));
        assert!(!a.contains("Edit"));
        assert!(a.contains("--model sonnet"));
        assert!(a.contains("--strict-mcp-config") && a.contains("--setting-sources project,local"));
        let w = build_args(None, Access::Write, false).join(" ");
        assert!(w.contains("acceptEdits") && !w.contains("--model"));
        assert!(!w.contains("--strict-mcp-config"), "personal config kept when isolation is off");
    }
}
