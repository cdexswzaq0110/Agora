//! `agora-mcp` — a stdio MCP server that lets Claude Code / Codex use a running Agora:
//! ask the *other* model (or both) for a discussion, debate or plan; read a session; read the
//! daily tech digest; check remaining quota.
//!
//! It talks to Agora's local HTTP API (`AGORA_URL`, default `http://127.0.0.1:4717`), so the
//! desktop app or `agora-server` must be running. Through MCP, sessions are always read-only
//! (a workshop stops at the plan): writing files stays a decision made in Agora's own UI.
//!
//! Register (examples):
//!   claude mcp add agora -- "C:\path\to\agora-mcp.exe"
//!   # ~/.codex/config.toml
//!   [mcp_servers.agora]
//!   command = "C:\\path\\to\\agora-mcp.exe"

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    fn new() -> Self {
        let base = std::env::var("AGORA_URL").unwrap_or_else(|_| "http://127.0.0.1:4717".into()).trim_end_matches('/').to_string();
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("http client");
        Api { base, http }
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        let r = self.http.get(format!("{}{path}", self.base)).send().await.map_err(|e| format!("連不到 Agora（{}）：{e}。請先開啟 Agora。", self.base))?;
        let ok = r.status().is_success();
        let v: Value = r.json().await.map_err(|e| e.to_string())?;
        if ok {
            Ok(v)
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("Agora 回報錯誤").to_string())
        }
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("連不到 Agora（{}）：{e}。請先開啟 Agora。", self.base))?;
        let ok = r.status().is_success();
        let v: Value = r.json().await.map_err(|e| e.to_string())?;
        if ok {
            Ok(v)
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("Agora 回報錯誤").to_string())
        }
    }
}

fn tools() -> Value {
    json!([
        {
            "name": "agora_discuss",
            "description": "Ask Agora to have Claude and/or GPT (through their CLIs, on the user's subscriptions) answer, discuss, debate or plan a question, and return the verdict with an evidence grade. Read-only: nothing is written to disk. Use it for a second opinion from the other model family, or a structured debate before a decision.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The question or task, in full." },
                    "mode": { "type": "string", "enum": ["solo", "roundtable", "debate", "workshop"], "description": "solo = one model; roundtable = discussion + synthesis; debate = pro/con + cross-judging; workshop = proposals → critique → plan (plan only). Default debate." },
                    "providers": { "type": "array", "items": { "type": "string", "enum": ["claude", "codex"] }, "description": "Which CLIs take part. Default both." },
                    "project_path": { "type": "string", "description": "Optional project directory the models may read." },
                    "wait_seconds": { "type": "number", "description": "How long to wait for the verdict (default 900)." }
                },
                "required": ["question"]
            }
        },
        {
            "name": "agora_session",
            "description": "Read an Agora session (transcript + verdict) as Markdown.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }
        },
        {
            "name": "agora_digest",
            "description": "Read Agora's daily software-engineering digest (Hacker News, Lobsters, Reddit, GitHub Trending, dev.to, Hugging Face Papers), summarized in Traditional Chinese.",
            "inputSchema": { "type": "object", "properties": { "date": { "type": "string", "description": "YYYY-MM-DD or 'latest' (default)." } } }
        },
        {
            "name": "agora_usage",
            "description": "Remaining subscription quota for Claude and GPT (5-hour and weekly windows) as Agora last observed it.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn project_id(api: &Api, path: Option<&str>) -> Result<String, String> {
    let projects = api.get("/api/projects").await?;
    let list = projects.as_array().cloned().unwrap_or_default();
    if let Some(p) = path.filter(|p| !p.trim().is_empty()) {
        let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
        if let Some(found) = list.iter().find(|x| x.get("path").and_then(Value::as_str).is_some_and(|q| norm(q) == norm(p))) {
            return Ok(found["id"].as_str().unwrap_or_default().to_string());
        }
        let name = std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "MCP 專案".into());
        let created = api.post("/api/projects", json!({ "name": name, "path": p })).await?;
        return Ok(created["id"].as_str().unwrap_or_default().to_string());
    }
    list.first().and_then(|x| x["id"].as_str()).map(str::to_string).ok_or_else(|| "Agora 沒有任何專案".into())
}

async fn discuss(api: &Api, args: &Value) -> Result<String, String> {
    let question = args.get("question").and_then(Value::as_str).filter(|q| !q.trim().is_empty()).ok_or("缺少 question")?;
    let mode = args.get("mode").and_then(Value::as_str).unwrap_or("debate");
    if !["solo", "roundtable", "debate", "workshop"].contains(&mode) {
        return Err(format!("不支援的 mode：{mode}"));
    }
    let providers: Vec<String> = args
        .get("providers")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| vec!["claude".into(), "codex".into()]);
    let pid = project_id(api, args.get("project_path").and_then(Value::as_str)).await?;
    let participants: Vec<Value> = providers.iter().map(|p| json!({ "provider": p })).collect();
    let spec = json!({
        "project_id": pid,
        "mode": mode,
        "participants": participants,
        "prompt": question,
        "access": "read_only",
        "plan_only": true,
        "auto_approve": true,
    });
    let rec = api.post("/api/sessions", spec).await?;
    let id = rec["id"].as_str().ok_or("Agora 沒有回傳議題 id")?.to_string();
    let wait = args.get("wait_seconds").and_then(Value::as_f64).unwrap_or(900.0).clamp(10.0, 3600.0);
    let t0 = Instant::now();
    loop {
        let r = api.get(&format!("/api/sessions/{id}")).await?;
        let status = r["status"].as_str().unwrap_or("");
        if status != "running" && status != "awaiting_approval" {
            return Ok(render(&api.base, &r));
        }
        if t0.elapsed().as_secs_f64() > wait {
            return Ok(format!("議題 {id} 仍在進行（{}）。稍後用 agora_session 讀取結果，或打開 {}/#/s/{id}", r["phase"].as_str().unwrap_or("進行中"), api.base));
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

fn render(base: &str, r: &Value) -> String {
    let mut s = format!("# {}\n\n狀態：{}\n", r["title"].as_str().unwrap_or(""), r["status"].as_str().unwrap_or(""));
    if let Some(v) = r.get("verdict").filter(|v| !v.is_null()) {
        s.push_str(&format!(
            "\n## 結論（證據等級：{}）\n**{}**\n\n{}\n",
            v["grade"].as_str().unwrap_or(""),
            v["headline"].as_str().unwrap_or(""),
            v["summary"].as_str().unwrap_or("")
        ));
        if let Some(items) = v["open_issues"].as_array().filter(|a| !a.is_empty()) {
            s.push_str("\n待處理／待驗證：\n");
            for i in items {
                s.push_str(&format!("- {}\n", i.as_str().unwrap_or("")));
            }
        }
    }
    if let Some(e) = r["error"].as_str() {
        s.push_str(&format!("\n錯誤：{e}\n"));
    }
    s.push_str(&format!("\n完整紀錄：{base}/#/s/{}\n", r["id"].as_str().unwrap_or("")));
    s
}

async fn call_tool(api: &Api, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "agora_discuss" => discuss(api, args).await,
        "agora_session" => {
            let id = args.get("id").and_then(Value::as_str).ok_or("缺少 id")?;
            let r = api.http.get(format!("{}/api/sessions/{id}/export.md", api.base)).send().await.map_err(|e| e.to_string())?;
            if !r.status().is_success() {
                return Err(format!("找不到議題 {id}"));
            }
            let text = r.text().await.map_err(|e| e.to_string())?;
            Ok(text.chars().take(30_000).collect())
        }
        "agora_digest" => {
            let date = args.get("date").and_then(Value::as_str).unwrap_or("latest");
            let d = api.get(&format!("/api/digests/{date}")).await?;
            let mut s = format!("# 每日技術情報 {}\n\n{}\n\n## 項目\n", d["date"].as_str().unwrap_or(""), d["markdown"].as_str().unwrap_or(""));
            for i in d["items"].as_array().cloned().unwrap_or_default() {
                s.push_str(&format!("[{}] {} — {}\n", i["n"], i["title"].as_str().unwrap_or(""), i["url"].as_str().unwrap_or("")));
            }
            Ok(s)
        }
        "agora_usage" => {
            let qs = api.get("/api/usage").await?;
            let mut s = String::from("剩餘額度（Agora 最後一次觀測）：\n");
            for q in qs.as_array().cloned().unwrap_or_default() {
                s.push_str(&format!("- {}:", q["provider"].as_str().unwrap_or("")));
                for w in q["windows"].as_array().cloned().unwrap_or_default() {
                    s.push_str(&format!(" {} 已用 {:.0}%;", w["label"].as_str().unwrap_or(""), w["used_percent"].as_f64().unwrap_or(0.0)));
                }
                s.push('\n');
            }
            Ok(s)
        }
        other => Err(format!("未知的工具：{other}")),
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[tokio::main]
async fn main() {
    let api = Api::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            let _ = writeln!(out, "{}", error(&Value::Null, -32700, "parse error"));
            let _ = out.flush();
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let Some(id) = id else { continue }; // notifications (e.g. notifications/initialized)
        let resp = match method {
            "initialize" => {
                let version = msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
                reply(&id, json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "agora", "title": "Agora", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Agora runs Claude and GPT together through their CLIs on the user's subscriptions. Use agora_discuss for a second opinion or a debate; it is read-only."
                }))
            }
            "ping" => reply(&id, json!({})),
            "tools/list" => reply(&id, json!({ "tools": tools() })),
            "tools/call" => {
                let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
                let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
                match call_tool(&api, name, &args).await {
                    Ok(text) => reply(&id, json!({ "content": [{ "type": "text", "text": text }], "isError": false })),
                    Err(e) => reply(&id, json!({ "content": [{ "type": "text", "text": e }], "isError": true })),
                }
            }
            _ => error(&id, -32601, "method not found"),
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}
