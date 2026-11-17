//! API + trust-model tests through the real router (mock providers, no network listener).

use agora_core::provider::{mock::MockProvider, Provider};
use agora_core::{random_hex, Engine, Family, Store};
use agora_server::{router, AppState};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

fn state() -> AppState {
    let dir = std::env::temp_dir().join(format!("agora-api-{}", random_hex(4)));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(MockProvider::new("mock-claude", Family::Claude).with_delay(Duration::from_millis(2))),
        Arc::new(MockProvider::new("mock-gpt", Family::Gpt).with_delay(Duration::from_millis(2))),
    ];
    AppState::new(Engine::with_providers(Store::open(dir).unwrap(), providers).unwrap(), 4717)
}

fn app(st: &AppState, peer: &str) -> Router {
    let addr: SocketAddr = peer.parse().unwrap();
    router(st.clone()).layer(MockConnectInfo(addr))
}

const LOCAL: &str = "127.0.0.1:50000";
const PHONE: &str = "192.168.1.20:50000";

async fn call(app: Router, req: Request<Body>) -> (StatusCode, Option<String>, Value) {
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let cookie = resp.headers().get(header::SET_COOKIE).map(|v| v.to_str().unwrap().to_string());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, cookie, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn get(path: &str, host: &str) -> Request<Body> {
    Request::get(path).header(header::HOST, host).body(Body::empty()).unwrap()
}

fn post(path: &str, host: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, format!("http://{host}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn local_is_trusted_and_phone_must_pair() {
    let st = state();
    let (s, _, v) = call(app(&st, LOCAL), get("/api/whoami", "127.0.0.1:4717")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["local"], true);

    let (s, _, _) = call(app(&st, PHONE), get("/api/providers", "192.168.1.5:4718")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, _, _) = call(app(&st, PHONE), post("/api/pair", "192.168.1.5:4718", json!({"code": "000000x"}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (code, _) = st.pairing.current();
    let (s, cookie, _) = call(app(&st, PHONE), post("/api/pair", "192.168.1.5:4718", json!({"code": code, "name": "iPhone"}))).await;
    assert_eq!(s, StatusCode::OK);
    let cookie = cookie.expect("pairing sets a cookie");
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    let token = cookie.split(';').next().unwrap().to_string();

    let req = Request::get("/api/providers").header(header::HOST, "192.168.1.5:4718").header(header::COOKIE, &token).body(Body::empty()).unwrap();
    let (s, _, v) = call(app(&st, PHONE), req).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v.as_array().unwrap().len(), 2);

    // Paired phones still cannot change machine-level settings.
    let req = Request::patch("/api/settings")
        .header(header::HOST, "192.168.1.5:4718")
        .header(header::COOKIE, &token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"claude_path": "C:/evil.exe"}).to_string()))
        .unwrap();
    let (s, _, _) = call(app(&st, PHONE), req).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn assistant_answers_rejects_parallel_and_requires_pairing() {
    let st = state();
    let body = json!({"message":"請簡短回答"});
    let (status, _, answer) = call(app(&st, LOCAL), post("/api/assistant/ask", "127.0.0.1:4717", body)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["text"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(answer["provider"].as_str().unwrap().starts_with("mock-"));

    let slow_app = app(&st, LOCAL);
    let first = tokio::spawn(async move {
        call(slow_app, post("/api/assistant/ask", "127.0.0.1:4717", json!({"message":"[mock:slow] 慢慢回答"}))).await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let (status, _, _) = call(app(&st, LOCAL), post("/api/assistant/ask", "127.0.0.1:4717", json!({"message":"第二題"}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = first.await.unwrap();
    assert_eq!(status, StatusCode::OK);

    let (status, _, _) = call(app(&st, PHONE), post("/api/assistant/ask", "192.168.1.5:4718", json!({"message":"未配對"}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn assistant_rejects_long_messages_and_invalid_roles() {
    let st = state();
    let (status, _, body) = call(app(&st, LOCAL), post("/api/assistant/ask", "127.0.0.1:4717", json!({"message":"字".repeat(2001)}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("2000"));

    let body = json!({"message":"你好", "history":[{"role":"system", "text":"越權"}]});
    let (status, _, _) = call(app(&st, LOCAL), post("/api/assistant/ask", "127.0.0.1:4717", body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn wait_status(st: &AppState, id: &str, want: &str) -> Value {
    for _ in 0..400 {
        let (_, _, v) = call(app(st, LOCAL), get(&format!("/api/sessions/{id}"), "127.0.0.1:4717")).await;
        if v["status"] == want {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("session {id} never reached {want}");
}

#[tokio::test]
async fn team_workflow_api_is_idempotent_and_export_is_local_only() {
    let st = state();
    let h = "127.0.0.1:4717";
    let (s, _, info) = call(app(&st, LOCAL), get("/api/team", h)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(info["roles"].as_array().unwrap().len(), 18);
    assert_eq!(info["depths"]["standard"]["general"]["calls"], 9);

    let (_, _, projects) = call(app(&st, LOCAL), get("/api/projects", h)).await;
    let project = projects[0]["id"].as_str().unwrap().to_string();
    let body = json!({ "project_id": project, "idea": "幫團隊排值班的小工具", "depth": "quick", "claude": "mock-claude", "gpt": "mock-gpt" });
    let (s, _, wf) = call(app(&st, LOCAL), post("/api/workflows", h, body)).await;
    assert_eq!(s, StatusCode::OK, "{wf}");
    let id = wf["id"].as_str().unwrap().to_string();
    assert_eq!(wf["next"], "discovery");

    // Two starts → one session; an unknown stage or workflow is a 404.
    let start = format!("/api/workflows/{id}/stages/discovery/start");
    let (s, _, a) = call(app(&st, LOCAL), post(&start, h, json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let (_, _, b) = call(app(&st, LOCAL), post(&start, h, json!({}))).await;
    assert_eq!(a["id"], b["id"]);
    let (s, _, _) = call(app(&st, LOCAL), post(&format!("/api/workflows/{id}/stages/deploy/start"), h, json!({}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = call(app(&st, LOCAL), get("/api/workflows/w-ffff", h)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The stage is approved through the session's own gate.
    let sid = a["id"].as_str().unwrap().to_string();
    wait_status(&st, &sid, "awaiting_approval").await;
    let (s, _, _) = call(app(&st, LOCAL), post(&format!("/api/sessions/{sid}/decide"), h, json!({ "action": "approve" }))).await;
    assert_eq!(s, StatusCode::OK);
    wait_status(&st, &sid, "done").await;
    let mut view = Value::Null;
    for _ in 0..200 {
        view = call(app(&st, LOCAL), get(&format!("/api/workflows/{id}"), h)).await.2;
        if view["stages"][0]["status"] == "approved" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(view["stages"][0]["status"], "approved", "{view}");
    assert_eq!(view["next"], "feasibility");

    // A session cannot smuggle in a team stage through the public endpoint.
    let stage = b["spec"]["stage"].clone();
    let direct = json!({ "project_id": project, "mode": "workshop", "participants": [{ "provider": "mock-claude" }], "prompt": "x", "stage": stage });
    let (s, _, e) = call(app(&st, LOCAL), post("/api/sessions", h, direct)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");

    // Export writes into the project: this computer only.
    let (s, _, _) = call(app(&st, PHONE), post(&format!("/api/workflows/{id}/export"), "192.168.1.5:4718", json!({}))).await;
    assert!(s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN, "{s}");
    let (s, _, out) = call(app(&st, LOCAL), post(&format!("/api/workflows/{id}/export"), h, json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{out}");
    assert!(out["paths"][0].as_str().unwrap().ends_with("01-discovery-r1.md"));
}

#[tokio::test]
async fn rebinding_and_csrf_are_refused() {
    let st = state();
    // DNS rebinding: loopback peer, foreign Host → treated as remote → unpaired.
    let (s, _, _) = call(app(&st, LOCAL), get("/api/projects", "evil.example:4717")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let mut req = post("/api/sessions", "127.0.0.1:4717", json!({}));
    req.headers_mut().insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    let (s, _, _) = call(app(&st, LOCAL), req).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let req = Request::post("/api/sessions").header(header::HOST, "127.0.0.1:4717").header(header::CONTENT_TYPE, "text/plain").body(Body::from("{}")).unwrap();
    let (s, _, _) = call(app(&st, LOCAL), req).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn session_lifecycle_over_http() {
    let st = state();
    let (_, _, projects) = call(app(&st, LOCAL), get("/api/projects", "127.0.0.1:4717")).await;
    let pid = projects[0]["id"].as_str().unwrap().to_string();
    let body = json!({
        "project_id": pid,
        "mode": "debate",
        "participants": [{"provider": "mock-claude"}, {"provider": "mock-gpt"}],
        "prompt": "要用 Rust 還是 Go 寫 CLI？"
    });
    let (s, _, rec) = call(app(&st, LOCAL), post("/api/sessions", "127.0.0.1:4717", body)).await;
    assert_eq!(s, StatusCode::OK, "{rec}");
    let id = rec["id"].as_str().unwrap().to_string();
    let mut last = Value::Null;
    for _ in 0..200 {
        let (_, _, v) = call(app(&st, LOCAL), get(&format!("/api/sessions/{id}"), "127.0.0.1:4717")).await;
        if v["status"] != "running" {
            last = v;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(last["status"], "done", "{last}");
    assert_eq!(last["verdict"]["outcome"], "judged");

    let (s, _, list) = call(app(&st, LOCAL), get(&format!("/api/sessions?project={pid}"), "127.0.0.1:4717")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn web_ui_is_served_with_csp() {
    let st = state();
    let resp = app(&st, LOCAL).oneshot(get("/", "127.0.0.1:4717")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp.headers().get("content-security-policy").unwrap().to_str().unwrap();
    assert!(csp.contains("script-src 'self'"));
    let resp = app(&st, LOCAL).oneshot(get("/api/nope", "127.0.0.1:4717")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v2_endpoints_templates_digests_experiments_and_local_only_exports() {
    let st = state();
    let (s, _, t) = call(app(&st, LOCAL), get("/api/templates", "127.0.0.1:4717")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(t.as_array().unwrap().iter().any(|x| x["id"] == "workshop-feature"));

    let (s, _, d) = call(app(&st, LOCAL), get("/api/digests", "127.0.0.1:4717")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["dates"].as_array().unwrap().len(), 0);
    let (s, _, _) = call(app(&st, LOCAL), get("/api/digests/latest", "127.0.0.1:4717")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, _, projects) = call(app(&st, LOCAL), get("/api/projects", "127.0.0.1:4717")).await;
    let pid = projects[0]["id"].as_str().unwrap().to_string();
    let body = json!({"spec": {"project_id": pid, "mode": "solo", "participants": [{"provider": "mock-claude"}, {"provider": "mock-gpt"}], "prompt": "比較"}, "modes": ["solo", "debate"]});
    let (s, _, x) = call(app(&st, LOCAL), post("/api/experiments", "127.0.0.1:4717", body)).await;
    assert_eq!(s, StatusCode::OK, "{x}");

    // A finished solo session: export as Markdown works from anywhere paired, but writing
    // files into the project (ADR/lesson) is local-only.
    let (_, _, rec) = call(app(&st, LOCAL), post("/api/sessions", "127.0.0.1:4717", json!({"project_id": pid, "mode": "solo", "participants": [{"provider": "mock-claude"}], "prompt": "匯出測試"}))).await;
    let id = rec["id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        let (_, _, v) = call(app(&st, LOCAL), get(&format!("/api/sessions/{id}"), "127.0.0.1:4717")).await;
        if v["status"] != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let resp = app(&st, LOCAL).oneshot(get(&format!("/api/sessions/{id}/export.md"), "127.0.0.1:4717")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("content-disposition").unwrap().to_str().unwrap().contains("attachment"));

    let (code, _) = st.pairing.current();
    let (_, cookie, _) = call(app(&st, PHONE), post("/api/pair", "192.168.1.5:4718", json!({"code": code}))).await;
    let token = cookie.unwrap().split(';').next().unwrap().to_string();
    let mut req = post(&format!("/api/sessions/{id}/export"), "192.168.1.5:4718", json!({"kind": "adr"}));
    req.headers_mut().insert(header::COOKIE, token.parse().unwrap());
    let (s, _, _) = call(app(&st, PHONE), req).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Not a workshop at the gate.
    let (s, _, _) = call(app(&st, LOCAL), post(&format!("/api/sessions/{id}/decide"), "127.0.0.1:4717", json!({"action": "approve"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // QR is local-only.
    let req = Request::get("/api/pairing/qr.svg").header(header::HOST, "192.168.1.5:4718").header(header::COOKIE, &token).body(Body::empty()).unwrap();
    let (s, _, _) = call(app(&st, PHONE), req).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[test]
fn mcp_server_handshake_and_tool_list() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_agora-mcp"))
        .env("AGORA_URL", "http://127.0.0.1:9")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut send = |v: serde_json::Value| writeln!(stdin, "{v}").unwrap();
    let mut line = String::new();

    send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}));
    out.read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["result"]["serverInfo"]["name"], "agora");
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");

    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    line.clear();
    out.read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["agora_discuss", "agora_session", "agora_digest", "agora_usage"]);

    // Agora not running → a tool error, not a crash.
    send(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "agora_usage", "arguments": {}}}));
    line.clear();
    out.read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["result"]["isError"], true);
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("連不到 Agora"));
    drop(stdin);
    let _ = child.wait();
}


#[tokio::test]
async fn v04_remember_bodies_digest_state_and_pairing_rules() {
    let st = state();
    let (_, _, projects) = call(app(&st, LOCAL), get("/api/projects", "127.0.0.1:4717")).await;
    let pid = projects[0]["id"].as_str().unwrap().to_string();
    let (_, _, rec) = call(app(&st, LOCAL), post("/api/sessions", "127.0.0.1:4717", json!({"project_id": pid, "mode": "solo", "participants": [{"provider": "mock-claude"}], "prompt": "記憶"}))).await;
    let id = rec["id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        let (_, _, v) = call(app(&st, LOCAL), get(&format!("/api/sessions/{id}"), "127.0.0.1:4717")).await;
        if v["status"] != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let path = format!("/api/sessions/{id}/remember");
    // An empty JSON request (pre-v0.4 clients) still works. A POST without the JSON content
    // type never gets here: the CSRF guard answers 415 first.
    let req = Request::post(&path)
        .header(header::HOST, "127.0.0.1:4717")
        .header(header::ORIGIN, "http://127.0.0.1:4717")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .unwrap();
    let (s, _, v) = call(app(&st, LOCAL), req).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _, _) = call(app(&st, LOCAL), post(&path, "127.0.0.1:4717", json!({}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, v) = call(app(&st, LOCAL), post(&path, "127.0.0.1:4717", json!({"line": "一句\n話"}))).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["memory"].as_str().unwrap().lines().last().unwrap().contains("] 一句 話【"));
    for bad in [json!({"line": "\n\n"}), json!({"line": "長".repeat(301)}), json!({"line": 3})] {
        let (s, _, _) = call(app(&st, LOCAL), post(&path, "127.0.0.1:4717", bad)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    let (s, _, v) = call(app(&st, LOCAL), get("/api/digests/state", "127.0.0.1:4717")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["unread"], false);
    let (s, _, _) = call(app(&st, LOCAL), post("/api/digests/2026-11-16/read", "127.0.0.1:4717", json!({"generated_at": 1}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no such digest version");

    // Unpaired devices get nothing from the new endpoints.
    let (s, _, _) = call(app(&st, PHONE), get("/api/digests/state", "192.168.1.5:4718")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = call(app(&st, PHONE), post(&path, "192.168.1.5:4718", json!({"line": "x"}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn skills_are_listed_verified_and_readonly_proof_is_local_only() {
    let st = state();
    let h = "127.0.0.1:4717";
    let (s, _, v) = call(app(&st, LOCAL), get("/api/skills", h)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true, "{v}");
    let core: Vec<&Value> = v["skills"].as_array().unwrap().iter().filter(|x| x["tier"] == "core").collect();
    assert_eq!(core.len(), 9);
    assert!(core.iter().all(|x| x["ok"] == true && x["sha256"].as_str().unwrap().len() == 64));
    let spec = core.iter().find(|x| x["name"] == "se-spec-review").unwrap();
    assert!(spec["used_by"].as_array().unwrap().iter().any(|u| u == "工作坊・規格審查"));

    let (s, _, _) = call(app(&st, PHONE), post("/api/skills/prove-readonly", "192.168.1.5:4718", json!({ "provider": "mock-gpt" }))).await;
    assert!(s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN);
    let (s, _, proof) = call(app(&st, LOCAL), post("/api/skills/prove-readonly", h, json!({ "provider": "mock-gpt" }))).await;
    assert_eq!(s, StatusCode::OK, "{proof}");
    assert_eq!(proof["ok"], true, "a read-only mock never writes: {proof}");
    let (_, _, providers) = call(app(&st, LOCAL), get("/api/providers", h)).await;
    let gpt = providers.as_array().unwrap().iter().find(|p| p["id"] == "mock-gpt").unwrap();
    assert_eq!(gpt["readonly_proven"], true);
    assert_eq!(gpt["readonly_proof"]["ok"], true);
}
