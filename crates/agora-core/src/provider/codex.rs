//! Codex CLI provider: `codex exec --json`, plus `codex app-server` (JSON-RPC over stdio)
//! for account type and quota — both free, neither spends quota.
//!
//! Billing guard: before every turn, `account/read` must report `account.type == "chatgpt"`.
//! If the account is `apiKey` (or cannot be confirmed), the turn is refused — fail closed.
//!
//! Gotcha (docs/lessons/0002): the model configured in `~/.codex/config.toml` may not be
//! available on the user's ChatGPT plan (`model_not_found`). Agora passes `-m` explicitly when
//! the user picks a model, and remembers rejected models so the UI can mark them.

use anyhow::{anyhow, bail, Context};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::proc;
use super::resolve::{codex_home, resolve_codex};
use super::stream::{run_capture, run_streaming, LineAction, RunResult};
use super::{Health, ModelOption, Provider, ProviderEvent, Sink, TurnOutput, TurnRequest};
use crate::model::{now_ms, Access, Family, TokenUsage, ToolNote};
use crate::quota::{from_codex_rate_limits, QuotaSnapshot};

pub const LOGIN_FIX: &str = "在終端機執行：codex login（選 Sign in with ChatGPT，不要用 API key）";

#[derive(Clone, Debug)]
pub struct AccountInfo {
    /// `chatgpt`, `apiKey`, `amazonBedrock`, … or `none` when logged out.
    pub kind: String,
    pub plan: Option<String>,
}

#[derive(Clone)]
struct Probe {
    at: u64,
    account: Result<AccountInfo, String>,
    quota: Option<QuotaSnapshot>,
}

pub struct CodexCli {
    id: String,
    exe_override: Option<String>,
    probe_cache: Mutex<Option<Probe>>,
}

impl CodexCli {
    pub fn new(exe_override: Option<String>) -> Self {
        CodexCli { id: "codex".into(), exe_override, probe_cache: Mutex::new(None) }
    }

    fn exe(&self) -> Option<PathBuf> {
        resolve_codex(self.exe_override.as_deref())
    }

    /// One `app-server` round trip for account + rate limits, cached for `max_age_ms`.
    async fn probe(&self, max_age_ms: u64) -> Probe {
        if let Some(p) = self.probe_cache.lock().unwrap().clone() {
            if now_ms().saturating_sub(p.at) <= max_age_ms {
                return p;
            }
        }
        let probe = match self.exe() {
            None => Probe { at: now_ms(), account: Err("找不到 codex CLI".into()), quota: None },
            Some(exe) => match app_server_call(
                &exe,
                &[("account/read", json!({})), ("account/rateLimits/read", Value::Null)],
                Duration::from_secs(25),
            )
            .await
            {
                Err(e) => Probe { at: now_ms(), account: Err(format!("{e:#}")), quota: None },
                Ok(results) => {
                    let account = match results.get("account/read") {
                        Some(Ok(v)) => Ok(parse_account(v)),
                        Some(Err(e)) => Err(e.clone()),
                        None => Err("app-server 沒有回應 account/read".into()),
                    };
                    let quota = match results.get("account/rateLimits/read") {
                        Some(Ok(v)) => from_codex_rate_limits(v, &self.id, now_ms()),
                        _ => None,
                    };
                    Probe { at: now_ms(), account, quota }
                }
            },
        };
        *self.probe_cache.lock().unwrap() = Some(probe.clone());
        probe
    }
}

pub fn parse_account(result: &Value) -> AccountInfo {
    match result.get("account").filter(|a| !a.is_null()) {
        None => AccountInfo { kind: "none".into(), plan: None },
        Some(a) => AccountInfo {
            kind: a.get("type").and_then(Value::as_str).unwrap_or("unknown").to_string(),
            plan: a.get("planType").and_then(Value::as_str).map(str::to_string),
        },
    }
}

/// Spawn `codex app-server`, do the `initialize` handshake, send each `(method, params)`, and
/// collect each response by method name.
///
/// `params` is per-method schema, not optional by convention: `account/read` *requires* an
/// object (`{}`), `account/rateLimits/read` takes `null` (docs/lessons/0003). `Value::Null`
/// omits the field.
pub async fn app_server_call(
    exe: &Path,
    methods: &[(&str, Value)],
    timeout: Duration,
) -> anyhow::Result<HashMap<String, Result<Value, String>>> {
    let mut cmd = proc::command(exe);
    cmd.arg("app-server");
    let mut child = cmd.spawn().context("無法啟動 codex app-server")?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut lines = BufReader::new(stdout).lines();

    let work = async {
        let send = |v: Value| format!("{v}\n");
        stdin
            .write_all(send(json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "agora", "title": "Agora", "version": crate::VERSION}}})).as_bytes())
            .await?;
        stdin.flush().await?;
        let mut initialized = false;
        let mut results: HashMap<String, Result<Value, String>> = HashMap::new();
        let mut pending: HashMap<u64, String> = HashMap::new();
        while let Some(line) = lines.next_line().await? {
            let Ok(o) = serde_json::from_str::<Value>(&line) else { continue };
            let Some(id) = o.get("id").and_then(Value::as_u64) else { continue };
            if id == 1 && !initialized {
                if let Some(e) = o.get("error") {
                    bail!("app-server initialize 失敗：{e}");
                }
                initialized = true;
                stdin.write_all(send(json!({"method": "initialized"})).as_bytes()).await?;
                for (i, (m, params)) in methods.iter().enumerate() {
                    let rid = 2 + i as u64;
                    pending.insert(rid, m.to_string());
                    let mut req = json!({"id": rid, "method": m});
                    if !params.is_null() {
                        req["params"] = params.clone();
                    }
                    stdin.write_all(send(req).as_bytes()).await?;
                }
                stdin.flush().await?;
                continue;
            }
            if let Some(m) = pending.remove(&id) {
                let r = match (o.get("result"), o.get("error")) {
                    (_, Some(e)) => Err(e.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| e.to_string())),
                    (Some(r), None) => Ok(r.clone()),
                    _ => Err("空回應".into()),
                };
                results.insert(m, r);
                if pending.is_empty() {
                    return Ok(results);
                }
            }
        }
        bail!("app-server 提前結束")
    };
    let out = tokio::time::timeout(timeout, work).await;
    proc::kill_tree(&mut child).await;
    match out {
        Ok(r) => r,
        Err(_) => bail!("app-server 逾時"),
    }
}

pub fn build_args(model: Option<&str>, access: Access, cwd: &Path, clean_config: bool) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--ephemeral".into(),
        "-C".into(),
        cwd.display().to_string(),
        "-s".into(),
        match access {
            Access::ReadOnly => "read-only".into(),
            Access::Write => "workspace-write".into(),
        },
    ];
    if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
        a.push("-m".into());
        a.push(m.trim().into());
    }
    if clean_config {
        // No ~/.codex/config.toml: no user MCP servers/plugins, no unsupported default model.
        a.push("--ignore-user-config".into());
    }
    a.push("-".into()); // prompt from stdin
    a
}

/// `The 'gpt-x' model is not supported…` → `gpt-x`.
pub fn rejected_model_name(msg: &str) -> Option<String> {
    let start = msg.find('\'')? + 1;
    let end = start + msg[start..].find('\'')?;
    let name = &msg[start..end];
    (msg[end..].to_ascii_lowercase().starts_with("' model") && !name.is_empty()).then(|| name.to_string())
}

/// `model = "…"` from `~/.codex/config.toml` (top-level key only).
pub fn configured_model() -> Option<String> {
    let raw = std::fs::read_to_string(codex_home()?.join("config.toml")).ok()?;
    raw.lines().take_while(|l| !l.trim_start().starts_with('[')).find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "model").then(|| v.trim().trim_matches('"').to_string()).filter(|m| !m.is_empty())
    })
}

/// Turn a raw Codex error (often JSON-in-a-string) into something actionable.
pub fn friendly_error(raw: &str, model: Option<&str>) -> (String, bool) {
    let inner: Option<Value> = serde_json::from_str(raw).ok();
    let (msg, code) = match &inner {
        Some(v) => (
            v.pointer("/error/message").and_then(Value::as_str).unwrap_or(raw).to_string(),
            v.pointer("/error/code").and_then(Value::as_str).unwrap_or("").to_string(),
        ),
        None => (raw.to_string(), String::new()),
    };
    let lower = msg.to_ascii_lowercase();
    if code == "model_not_found" || lower.contains("model is not supported") || lower.contains("model_not_found") {
        let m = model.unwrap_or("config.toml 預設模型");
        return (format!("此模型（{m}）不支援你目前的 ChatGPT 方案：{msg}。請在模型選單換一個。"), true);
    }
    if lower.contains("usage limit") || lower.contains("rate limit") {
        return (format!("Codex 已達使用上限：{msg}"), false);
    }
    if lower.contains("not logged in") || lower.contains("unauthorized") || lower.contains("401") {
        return (format!("Codex 未登入或登入已過期：{msg}。{LOGIN_FIX}"), false);
    }
    (msg, false)
}

/// Pure JSONL parser for `codex exec --json`.
#[derive(Default)]
pub struct CodexJsonParser {
    model: Option<String>,
    pub messages: Vec<String>,
    pub error: Option<String>,
    pub usage: Option<TokenUsage>,
    pub completed: bool,
    pub unsupported_model: bool,
    pub rejected_model: Option<String>,
}

impl CodexJsonParser {
    pub fn new(model: Option<&str>) -> Self {
        CodexJsonParser { model: model.map(str::to_string), ..Default::default() }
    }

    pub fn feed(&mut self, line: &str) -> Vec<ProviderEvent> {
        let Ok(o) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        let mut out = Vec::new();
        let ty = o.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "item.started" | "item.completed" => {
                let item = o.get("item").unwrap_or(&Value::Null);
                let it = item.get("type").and_then(Value::as_str).unwrap_or("");
                match (ty, it) {
                    ("item.completed", "agent_message") => {
                        if let Some(t) = item.get("text").and_then(Value::as_str) {
                            let sep = if self.messages.is_empty() { "" } else { "\n\n" };
                            out.push(ProviderEvent::Delta(format!("{sep}{t}")));
                            self.messages.push(t.to_string());
                        }
                    }
                    ("item.started", "command_execution") => out.push(ProviderEvent::Tool(ToolNote {
                        name: "shell".into(),
                        detail: crate::compact::truncate_chars(item.get("command").and_then(Value::as_str).unwrap_or(""), 120),
                    })),
                    ("item.completed", "file_change") => {
                        let paths: Vec<String> = item
                            .get("changes")
                            .and_then(Value::as_array)
                            .map(|cs| cs.iter().filter_map(|c| c.get("path").and_then(Value::as_str).map(str::to_string)).collect())
                            .unwrap_or_default();
                        out.push(ProviderEvent::Tool(ToolNote { name: "edit".into(), detail: paths.join(", ") }));
                    }
                    ("item.started", "mcp_tool_call") => out.push(ProviderEvent::Tool(ToolNote {
                        name: format!("mcp:{}", item.get("tool").and_then(Value::as_str).unwrap_or("?")),
                        detail: item.get("server").and_then(Value::as_str).unwrap_or("").to_string(),
                    })),
                    ("item.started", "web_search") => out.push(ProviderEvent::Tool(ToolNote {
                        name: "web_search".into(),
                        detail: item.get("query").and_then(Value::as_str).unwrap_or("").to_string(),
                    })),
                    ("item.completed", "error") => {
                        // Non-fatal warnings (e.g. unsupported service tier).
                        if let Some(m) = item.get("message").and_then(Value::as_str) {
                            out.push(ProviderEvent::Warning(m.to_string()));
                        }
                    }
                    _ => {}
                }
            }
            "turn.completed" => {
                self.completed = true;
                self.error = None;
                if let Some(u) = o.get("usage") {
                    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    self.usage = Some(TokenUsage {
                        input_tokens: g("input_tokens"),
                        output_tokens: g("output_tokens"),
                        cached_input_tokens: g("cached_input_tokens"),
                    });
                }
            }
            "turn.failed" | "error" => {
                let raw = o
                    .pointer("/error/message")
                    .or_else(|| o.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Codex 回報錯誤");
                let (msg, unsupported) = friendly_error(raw, self.model.as_deref());
                if unsupported {
                    self.unsupported_model = true;
                    let inner = serde_json::from_str::<Value>(raw)
                        .ok()
                        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
                        .unwrap_or_else(|| raw.to_string());
                    self.rejected_model = rejected_model_name(&inner).or_else(|| self.model.clone());
                }
                self.error = Some(msg);
            }
            _ => {}
        }
        out
    }

    pub fn finish(self, run: &RunResult) -> TurnOutput {
        let streamed = self.messages.join("\n\n");
        let mut out = TurnOutput {
            usage: self.usage.clone(),
            unsupported_model: self.unsupported_model,
            rejected_model: self.rejected_model.clone(),
            ..Default::default()
        };
        if let Some(e) = &run.spawn_error {
            out.error = Some(format!("無法啟動 codex：{e}"));
        } else if run.cancelled {
            out.error = Some("已取消".into());
            out.text = streamed;
        } else if run.timed_out {
            out.error = Some("逾時，已終止 codex 程序".into());
            out.text = streamed;
        } else if let Some(e) = self.error.filter(|_| !self.completed) {
            out.error = Some(e);
            out.text = streamed;
        } else if self.completed {
            out.ok = true;
            out.text = self.messages.last().cloned().unwrap_or_default();
        } else {
            out.error = Some(format!(
                "codex 沒有完成這一輪（exit code {}）{}",
                run.exit_code.map_or("?".into(), |c| c.to_string()),
                if run.stderr_tail.trim().is_empty() { String::new() } else { format!("：{}", run.stderr_tail.trim()) }
            ));
            out.text = streamed;
        }
        out
    }
}

fn read_models_cache() -> Vec<ModelOption> {
    let Some(path) = codex_home().map(|h| h.join("models_cache.json")) else {
        return vec![];
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return vec![];
    };
    v.get("models")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
                .filter_map(|m| {
                    let id = m.get("slug").and_then(Value::as_str)?.to_string();
                    let label = m.get("display_name").and_then(Value::as_str).unwrap_or(&id).to_string();
                    Some(ModelOption { id, label })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl Provider for CodexCli {
    fn id(&self) -> &str {
        &self.id
    }
    fn family(&self) -> Family {
        Family::Gpt
    }
    fn display_name(&self) -> String {
        "GPT（Codex CLI）".into()
    }
    fn models(&self) -> Vec<ModelOption> {
        let label = match configured_model() {
            Some(m) => format!("CLI 預設（{m}）"),
            None => "CLI 預設（config.toml）".into(),
        };
        let mut v = vec![ModelOption { id: String::new(), label }];
        v.extend(read_models_cache());
        v
    }

    async fn health(&self) -> Health {
        let checked_at = now_ms();
        let Some(exe) = self.exe() else {
            return Health {
                ok: false,
                summary: "找不到 Codex CLI".into(),
                fix: Some("安裝：npm install -g @openai/codex，或在設定中指定 codex.exe 路徑".into()),
                version: None,
                account: None,
                executable: None,
                checked_at,
            };
        };
        let mut v = proc::command(&exe);
        v.arg("--version");
        let version = run_capture(v, Duration::from_secs(20)).await.ok();
        let probe = self.probe(0).await;
        let mut h = Health { ok: false, summary: String::new(), fix: None, version, account: None, executable: Some(exe.display().to_string()), checked_at };
        match probe.account {
            Err(e) => {
                h.summary = format!("無法讀取 Codex 帳號狀態：{e}");
                h.fix = Some("確認 codex 可以正常啟動（codex doctor）".into());
            }
            Ok(a) if a.kind == "none" => {
                h.summary = "Codex 尚未登入".into();
                h.fix = Some(LOGIN_FIX.into());
            }
            Ok(a) if a.kind != "chatgpt" => {
                h.summary = format!("Codex 目前以 {} 登入；Agora 只接受 ChatGPT 訂閱登入", a.kind);
                h.fix = Some(format!("codex logout 後重新 {LOGIN_FIX}"));
            }
            Ok(a) => {
                h.ok = true;
                h.summary = "已登入（ChatGPT 訂閱）".into();
                h.account = Some(format!("ChatGPT {}", a.plan.unwrap_or_default()).trim().to_string());
            }
        }
        h
    }

    async fn poll_quota(&self) -> Option<QuotaSnapshot> {
        self.probe(15_000).await.quota
    }

    async fn run(&self, req: TurnRequest, sink: Sink) -> TurnOutput {
        let Some(exe) = self.exe() else {
            return TurnOutput::fail("找不到 Codex CLI（npm install -g @openai/codex）");
        };
        // Billing guard — fail closed.
        let probe = self.probe(10 * 60_000).await;
        match &probe.account {
            Ok(a) if a.kind == "chatgpt" => {}
            Ok(a) => {
                let mut o = TurnOutput::fail(format!(
                    "計費保護：Codex 目前登入方式為「{}」，不是 ChatGPT 訂閱。Agora 不會用 API key 執行。{LOGIN_FIX}",
                    a.kind
                ));
                o.guard_tripped = true;
                return o;
            }
            Err(e) => {
                let mut o = TurnOutput::fail(format!("計費保護：無法確認 Codex 登入方式（{e}），為避免誤用 API key 計費，本輪不執行。"));
                o.guard_tripped = true;
                return o;
            }
        }
        let mut cmd = proc::command(&exe);
        cmd.args(build_args(req.model.as_deref(), req.access, &req.cwd, req.clean_config)).current_dir(&req.cwd);
        let mut parser = CodexJsonParser::new(req.model.as_deref());
        let res = run_streaming(cmd, req.prompt, req.timeout, req.cancel, |line| {
            for e in parser.feed(line) {
                let _ = sink.send(e);
            }
            LineAction::Continue
        })
        .await;
        let out = parser.finish(&res);
        // Quota moved; refresh it (free).
        *self.probe_cache.lock().unwrap() = None;
        if let Some(q) = self.probe(0).await.quota {
            let _ = sink.send(ProviderEvent::Quota(q));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_not_found_is_flagged_and_readable() {
        // Shape recorded from codex-cli 0.147 with an unsupported configured model.
        let lines = [
            r#"{"type":"thread.started","thread_id":"t"}"#,
            r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Configured service tier `priority` is not advertised"}}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"error","message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-x' model is not supported when using Codex with a ChatGPT account.\",\"code\":\"model_not_found\"}}"}"#,
            r#"{"type":"turn.failed","error":{"message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-x' model is not supported when using Codex with a ChatGPT account.\",\"code\":\"model_not_found\"}}"}}"#,
        ];
        let mut p = CodexJsonParser::new(Some("gpt-x"));
        let mut warnings = 0;
        for l in lines {
            warnings += p.feed(l).iter().filter(|e| matches!(e, ProviderEvent::Warning(_))).count();
        }
        assert_eq!(warnings, 1);
        let out = p.finish(&RunResult { exit_code: Some(1), ..Default::default() });
        assert!(!out.ok);
        assert!(out.unsupported_model);
        assert_eq!(out.rejected_model.as_deref(), Some("gpt-x"));
        let err = out.error.unwrap();
        assert!(err.contains("gpt-x") && err.contains("換一個"), "{err}");
    }

    #[test]
    fn rejected_model_is_named_even_when_none_was_requested() {
        let msg = "The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account.";
        assert_eq!(rejected_model_name(msg).as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(rejected_model_name("It's fine"), None);
        let mut p = CodexJsonParser::new(None);
        p.feed(r#"{"type":"turn.failed","error":{"message":"{\"error\":{\"message\":\"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account.\",\"code\":\"model_not_found\"}}"}}"#);
        assert_eq!(p.rejected_model.as_deref(), Some("gpt-6.1-sol"));
    }

    #[test]
    fn successful_turn_uses_last_agent_message_and_usage() {
        let lines = [
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"我先看看檔案"}}"#,
            r#"{"type":"item.started","item":{"type":"command_execution","command":"rg TODO"}}"#,
            r#"{"type":"item.completed","item":{"type":"file_change","changes":[{"path":"a.rs","kind":"update"}]}}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"完成：已修改 a.rs"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20}}"#,
        ];
        let mut p = CodexJsonParser::new(None);
        let mut tools = 0;
        for l in lines {
            tools += p.feed(l).iter().filter(|e| matches!(e, ProviderEvent::Tool(_))).count();
        }
        assert_eq!(tools, 2);
        let out = p.finish(&RunResult { exit_code: Some(0), ..Default::default() });
        assert!(out.ok);
        assert_eq!(out.text, "完成：已修改 a.rs");
        assert_eq!(out.usage.unwrap().cached_input_tokens, 40);
    }

    #[test]
    fn account_parsing_distinguishes_api_key_from_chatgpt() {
        assert_eq!(parse_account(&json!({"account": {"type": "apiKey"}})).kind, "apiKey");
        let c = parse_account(&json!({"account": {"type": "chatgpt", "planType": "plus", "email": null}}));
        assert_eq!((c.kind.as_str(), c.plan.as_deref()), ("chatgpt", Some("plus")));
        assert_eq!(parse_account(&json!({"account": null, "requiresOpenaiAuth": true})).kind, "none");
    }

    #[test]
    fn args_pick_sandbox_from_access_and_read_prompt_from_stdin() {
        let a = build_args(Some("gpt-6-sol"), Access::ReadOnly, Path::new("C:/p"), true).join(" ");
        assert!(a.contains("-s read-only") && a.contains("-m gpt-6-sol") && a.ends_with(" -"));
        assert!(a.contains("--ignore-user-config"));
        let w = build_args(None, Access::Write, Path::new("C:/p"), false).join(" ");
        assert!(w.contains("-s workspace-write") && !w.contains(" -m ") && !w.contains("--ignore-user-config"));
    }
}
