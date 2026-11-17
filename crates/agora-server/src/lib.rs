//! Agora HTTP server: the JSON API, Server-Sent Events, and the embedded web UI.
//!
//! One process serves every client — the Tauri window, a desktop browser, and phones (over
//! LAN or Tailscale). Trust model:
//! - **Local**: loopback peer + `Host: 127.0.0.1|localhost` + no proxy headers → trusted.
//! - **Everything else** (LAN, `tailscale serve`, DNS-rebinding attempts) needs the
//!   `agora_token` cookie obtained by entering the 6-digit pairing code shown on the desktop.
//! - State-changing requests must be same-origin JSON (CSRF / rebinding defence).

mod auth;
mod team_api;
mod v2;

use team_api::*;
use v2::*;

use agora_core::engine::GateDecision;
use agora_core::scheduler::Schedule;
use agora_core::templates::Template;
use agora_core::{AssistantError, Engine, Event, Mode, SessionSpec, Status};
use axum::{
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, patch, post},
    Json, Router,
};
use futures_util::{stream, Stream, StreamExt};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};

pub use auth::Pairing;

#[derive(RustEmbed)]
#[folder = "../../web/"]
struct Assets;

struct LanListener {
    port: u16,
    stop: oneshot::Sender<()>,
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub pairing: Arc<Pairing>,
    lan: Arc<Mutex<Option<LanListener>>>,
    pub port: u16,
}

impl AppState {
    pub fn new(engine: Arc<Engine>, port: u16) -> Self {
        AppState { engine, pairing: Arc::new(Pairing::new()), lan: Arc::new(Mutex::new(None)), port }
    }

    /// Start (or restart) the all-interfaces listener used by phones on the LAN.
    pub async fn start_lan(&self) -> anyhow::Result<u16> {
        self.stop_lan().await;
        let port = self.engine.settings().lan_port;
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
        let (tx, rx) = oneshot::channel::<()>();
        let app = router(self.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        *self.lan.lock().await = Some(LanListener { port, stop: tx });
        Ok(port)
    }

    pub async fn stop_lan(&self) {
        if let Some(l) = self.lan.lock().await.take() {
            let _ = l.stop.send(());
        }
    }

    async fn lan_port(&self) -> Option<u16> {
        self.lan.lock().await.as_ref().map(|l| l.port)
    }
}

/// Serve on an already-bound loopback listener until the process exits.
pub async fn serve(state: AppState, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    if state.engine.settings().lan_enabled {
        if let Err(e) = state.start_lan().await {
            eprintln!("Agora: 無法開啟區網連線埠：{e}");
        }
    }
    let app = router(state);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

/// Health at startup, then quota every 5 minutes (free for Codex; Claude is passive) and
/// health every 30 minutes.
pub fn spawn_background(engine: Arc<Engine>) {
    agora_core::scheduler::spawn(engine.clone());
    tokio::spawn(async move {
        engine.refresh_health().await;
        engine.refresh_quotas().await;
        let mut tick = 0u64;
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            tick += 1;
            engine.refresh_quotas().await;
            if tick % 6 == 0 {
                engine.refresh_health().await;
            }
        }
    });
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/whoami", get(whoami))
        .route("/api/pair", post(pair))
        .route("/api/health", get(health))
        .route("/api/providers", get(providers))
        .route("/api/providers/refresh", post(refresh_providers))
        .route("/api/usage", get(usage))
        .route("/api/usage/refresh", post(usage_refresh))
        .route("/api/usage/probe-claude", post(probe_claude))
        .route("/api/assistant/ask", post(assistant_ask))
        .route("/api/team", get(team_info))
        .route("/api/skills", get(list_skills))
        .route("/api/skills/prove-readonly", post(prove_readonly))
        .route("/api/workflows", get(list_workflows).post(create_workflow))
        .route("/api/workflows/{id}", get(get_workflow))
        .route("/api/workflows/{id}/stages/{kind}/start", post(start_stage))
        .route("/api/workflows/{id}/tasks/{task}/start", post(start_task))
        .route("/api/workflows/{id}/export", post(export_workflow))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", patch(update_project))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/events", get(session_events))
        .route("/api/sessions/{id}/interject", post(interject))
        .route("/api/sessions/{id}/cancel", post(cancel))
        .route("/api/sessions/{id}/remember", post(remember))
        .route("/api/sessions/{id}/decide", post(decide))
        .route("/api/sessions/{id}/diff", get(session_diff))
        .route("/api/sessions/{id}/workspace/apply", post(workspace_apply))
        .route("/api/sessions/{id}/workspace/discard", post(workspace_discard))
        .route("/api/sessions/{id}/export.md", get(export_md))
        .route("/api/sessions/{id}/export", post(export_file))
        .route("/api/templates", get(list_templates).post(save_template))
        .route("/api/templates/{id}", delete(delete_template))
        .route("/api/experiments", get(list_experiments).post(start_experiment))
        .route("/api/experiments/{id}", get(get_experiment))
        .route("/api/digests", get(list_digests))
        .route("/api/digests/state", get(digest_state))
        .route("/api/digests/{date}/read", post(mark_digest_read))
        .route("/api/digests/run", post(run_digest))
        .route("/api/digests/{date}", get(get_digest))
        .route("/api/schedules", get(list_schedules).post(save_schedule))
        .route("/api/schedules/{id}", delete(delete_schedule))
        .route("/api/schedules/{id}/run", post(run_schedule))
        .route("/api/usage/history", get(usage_history))
        .route("/api/usage/stats", get(usage_stats))
        .route("/api/pairing/qr.svg", get(pairing_qr))
        .route("/api/events", get(global_events))
        .route("/api/settings", get(get_settings).patch(patch_settings))
        .route("/api/pairing", get(pairing_info))
        .route("/api/pairing/rotate", post(pairing_rotate))
        .route("/api/pairing/devices/{id}", delete(revoke_device))
        .route("/api/network", post(set_network))
        .route("/api/network/tailscale", post(enable_tailscale))
        .fallback(static_handler)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

// ================================================================ errors

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::BAD_REQUEST, msg.into())
    }
    fn not_found(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::NOT_FOUND, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::bad(format!("{e:#}"))
    }
}

#[derive(Deserialize)]
struct AssistantAskBody {
    message: String,
    #[serde(default)]
    history: Vec<AssistantHistory>,
}

#[derive(Deserialize)]
struct AssistantHistory { role: String, text: String }

async fn assistant_ask(State(st): State<AppState>, Json(b): Json<AssistantAskBody>) -> Result<Json<Value>, ApiError> {
    let history = b.history.into_iter().map(|h| (h.role, h.text)).collect::<Vec<_>>();
    // The persona is a local-only extension file (git-ignored); the public build uses a neutral one.
    let persona = Assets::get("local/persona.txt").map(|f| String::from_utf8_lossy(&f.data).into_owned());
    st.engine.ask_assistant(&b.message, &history, persona.as_deref()).await.map(Json).map_err(|e| match e {
        AssistantError::Busy => ApiError(StatusCode::CONFLICT, e.to_string()),
        AssistantError::Invalid(_) => ApiError(StatusCode::BAD_REQUEST, e.to_string()),
        AssistantError::Unavailable(_) => ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
        AssistantError::Failed(_) => ApiError(StatusCode::BAD_GATEWAY, e.to_string()),
    }).map(|Json(a)| Json(json!(a)))
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// ================================================================ guard

#[derive(Clone, Debug)]
pub struct Caller {
    pub local: bool,
    pub device: Option<String>,
}

const PUBLIC_API: &[&str] = &["/api/whoami", "/api/pair"];
const LOCAL_ONLY_PREFIX: &[&str] = &["/api/pairing", "/api/network"];

async fn guard(State(st): State<AppState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let headers = req.headers().clone();
    let local = auth::is_local(addr, &headers);

    if method != Method::GET && method != Method::HEAD {
        if !auth::same_origin(&headers) {
            return ApiError(StatusCode::FORBIDDEN, "跨來源請求被拒絕".into()).into_response();
        }
        if matches!(method, Method::POST | Method::PATCH) && !auth::is_json(&headers) {
            return ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "請使用 application/json".into()).into_response();
        }
    }

    let device = if local { None } else { auth::device_for(&st.engine, &headers) };
    if path.starts_with("/api/") && !PUBLIC_API.contains(&path.as_str()) {
        if !local && device.is_none() {
            return ApiError(StatusCode::UNAUTHORIZED, "這個裝置尚未配對".into()).into_response();
        }
        let local_only = LOCAL_ONLY_PREFIX.iter().any(|p| path.starts_with(p)) || (path == "/api/settings" && method == Method::PATCH);
        if local_only && !local {
            return ApiError(StatusCode::FORBIDDEN, "這項設定只能在電腦本機操作".into()).into_response();
        }
    }
    req.extensions_mut().insert(Caller { local, device });
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    resp
}

// ================================================================ handlers

async fn whoami(State(st): State<AppState>, axum::Extension(c): axum::Extension<Caller>) -> Json<Value> {
    Json(json!({
        "local": c.local,
        "paired": c.local || c.device.is_some(),
        "device": c.device,
        "version": agora_core::VERSION,
        "demo": st.engine.providers().iter().any(|p| p.is_mock()),
        "extensions": local_extensions(),
    }))
}

/// Local-only UI extensions under `web/local/` (git-ignored, embedded only when the files exist
/// at build time). The page loads each one it is told about; nothing else references them.
fn local_extensions() -> Vec<&'static str> {
    let mut v = Vec::new();
    if Assets::get("local/pet.js").is_some() {
        v.push("pet");
    }
    v
}

#[derive(Deserialize)]
struct PairBody {
    code: String,
    #[serde(default)]
    name: Option<String>,
}

async fn pair(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<PairBody>) -> Result<Response, ApiError> {
    st.pairing.try_pair(&b.code).map_err(|e| ApiError(StatusCode::FORBIDDEN, e))?;
    let name = b.name.map(|n| n.trim().chars().take(40).collect::<String>()).filter(|n| !n.is_empty()).unwrap_or_else(|| "手機".into());
    let token = auth::register_device(&st.engine, &name)?;
    let secure = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let cookie = format!(
        "agora_token={token}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict{}",
        if secure { "; Secure" } else { "" }
    );
    let mut resp = Json(json!({ "ok": true, "device": name })).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).map_err(|e| ApiError::bad(e.to_string()))?);
    Ok(resp)
}

async fn health(State(st): State<AppState>) -> Json<Value> {
    // agora-mcp.exe ships next to the server/desktop executable.
    let mcp = std::env::current_exe().ok().and_then(|p| {
        let name = if cfg!(windows) { "agora-mcp.exe" } else { "agora-mcp" };
        let c = p.parent()?.join(name);
        c.is_file().then(|| c.display().to_string())
    });
    Json(json!({ "ok": true, "version": agora_core::VERSION, "data_dir": st.engine.store().root().display().to_string(), "mcp_path": mcp }))
}

async fn providers(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.providers_info()))
}

async fn refresh_providers(State(st): State<AppState>) -> Json<Value> {
    st.engine.refresh_health().await;
    st.engine.refresh_quotas().await;
    Json(json!(st.engine.providers_info()))
}

async fn usage(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.quotas()))
}

async fn usage_refresh(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.refresh_quotas().await))
}

async fn probe_claude(State(st): State<AppState>) -> ApiResult<Value> {
    let q = st.engine.probe_claude().await?;
    Ok(Json(json!({ "quota": q })))
}

async fn list_projects(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.projects()))
}

#[derive(Deserialize)]
struct NewProject {
    name: String,
    path: String,
}

async fn create_project(State(st): State<AppState>, Json(b): Json<NewProject>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.create_project(&b.name, &b.path)?)))
}

#[derive(Deserialize)]
struct ProjectPatch {
    name: Option<String>,
    path: Option<String>,
    memory: Option<String>,
}

async fn update_project(State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<ProjectPatch>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.update_project(&id, b.name, b.path, b.memory)?)))
}

#[derive(Deserialize)]
struct SessionQuery {
    project: Option<String>,
    q: Option<String>,
}

async fn list_sessions(State(st): State<AppState>, Query(q): Query<SessionQuery>) -> Json<Value> {
    Json(json!(st.engine.sessions(q.project.as_deref(), q.q.as_deref()).await))
}

async fn create_session(State(st): State<AppState>, Json(spec): Json<SessionSpec>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.start(spec).await?)))
}

async fn get_session(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    st.engine.session(&id).await.map(|r| Json(json!(r))).ok_or_else(|| ApiError::not_found("找不到專案"))
}

#[derive(Deserialize)]
struct TextBody {
    text: String,
}

async fn interject(State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<TextBody>) -> ApiResult<Value> {
    st.engine.interject(&id, &b.text).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn cancel(State(st): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": st.engine.cancel(&id).await }))
}

/// Body is optional: empty or `{}` keeps the pre-v0.4 line; `{"line": "…"}` stores the user's
/// (edited) sentence. Read raw so a body-less POST never fails before reaching the handler.
async fn remember(State(st): State<AppState>, Path(id): Path<String>, body: axum::body::Bytes) -> ApiResult<Value> {
    let line = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        let v: Value = serde_json::from_slice(&body).map_err(|_| ApiError::bad("需要 JSON"))?;
        match v.get("line") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(ApiError::bad("line 需要是字串")),
        }
    };
    Ok(Json(json!(st.engine.remember(&id, line.as_deref()).await?)))
}

fn sse_json(v: &impl serde::Serialize) -> SseEvent {
    SseEvent::default().data(serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
}

async fn session_events(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    // Subscribe before taking the snapshot so nothing falls between them; clients drop
    // duplicate deltas using `seq`.
    let rx = st.engine.subscribe();
    let snap = st.engine.session(&id).await.ok_or_else(|| ApiError::not_found("找不到專案"))?;
    let first = sse_json(&json!({ "type": "snapshot", "session": snap }));
    let live = BroadcastStream::new(rx).filter_map(move |r| {
        let id = id.clone();
        async move {
            match r {
                Ok(ev) if ev.session_id() == Some(id.as_str()) || matches!(ev, Event::Quota { .. }) => Some(Ok(sse_json(&ev))),
                Ok(_) => None,
                Err(BroadcastStreamRecvError::Lagged(_)) => Some(Ok(sse_json(&json!({ "type": "lagged" })))),
            }
        }
    });
    Ok(Sse::new(stream::once(async move { Ok(first) }).chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn global_events(State(st): State<AppState>) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = st.engine.subscribe();
    let first = sse_json(&json!({ "type": "hello", "quotas": st.engine.quotas() }));
    let live = BroadcastStream::new(rx).filter_map(|r| async move {
        match r {
            Ok(ev) if ev.is_global() => Some(Ok(sse_json(&ev))),
            Ok(_) => None,
            Err(_) => Some(Ok(sse_json(&json!({ "type": "lagged" })))),
        }
    });
    Sse::new(stream::once(async move { Ok(first) }).chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn get_settings(State(st): State<AppState>) -> Json<Value> {
    let s = st.engine.settings();
    Json(json!({
        "port": s.port,
        "lan_enabled": s.lan_enabled,
        "lan_port": s.lan_port,
        "claude_path": s.claude_path,
        "codex_path": s.codex_path,
        "default_models": s.default_models,
        "default_efforts": s.default_efforts,
        "unsupported_models": s.unsupported_models,
        "quota_stop_percent": s.quota_stop_percent,
        "demo_mode": s.demo_mode,
        "turn_timeout_secs": s.turn_timeout_secs,
        "verify_timeout_secs": s.verify_timeout_secs,
        "transcript_budget_chars": s.transcript_budget_chars,
        "auto_reroute": s.auto_reroute,
        "context_pack": s.context_pack,
        "isolate_cli_config": s.isolate_cli_config,
        "digest": s.digest,
        "data_dir": st.engine.store().root().display().to_string(),
    }))
}

fn opt_string(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn patch_settings(State(st): State<AppState>, Json(b): Json<Value>) -> Result<Json<Value>, ApiError> {
    let obj = b.as_object().ok_or_else(|| ApiError::bad("需要 JSON 物件"))?.clone();
    // Reject malformed values up front instead of silently dropping them inside the update.
    let mut efforts: Vec<(String, Option<agora_core::Effort>)> = Vec::new();
    if let Some(v) = obj.get("default_efforts") {
        let m = v.as_object().ok_or_else(|| ApiError::bad("default_efforts 需要是物件"))?;
        for (pid, e) in m {
            match opt_string(e) {
                None => efforts.push((pid.clone(), None)),
                Some(name) => match agora_core::Effort::parse(&name) {
                    Some(x) => efforts.push((pid.clone(), Some(x))),
                    None => return Err(ApiError::bad(format!("不認得的推理強度：{name}"))),
                },
            }
        }
    }
    if let Some(v) = obj.get("digest") {
        serde_json::from_value::<agora_core::digest::DigestSettings>(v.clone()).map_err(|e| ApiError::bad(format!("情報設定格式錯誤：{e}")))?;
    }
    st.engine.update_settings(|s| {
        for (pid, e) in &efforts {
            match e {
                Some(x) => {
                    s.default_efforts.insert(pid.clone(), *x);
                }
                None => {
                    s.default_efforts.remove(pid);
                }
            }
        }
        for (k, v) in &obj {
            match k.as_str() {
                "claude_path" => s.claude_path = opt_string(v),
                "codex_path" => s.codex_path = opt_string(v),
                "quota_stop_percent" => {
                    if let Some(x) = v.as_f64() {
                        s.quota_stop_percent = x;
                    }
                }
                "demo_mode" => {
                    if let Some(x) = v.as_bool() {
                        s.demo_mode = x;
                    }
                }
                "turn_timeout_secs" => {
                    if let Some(x) = v.as_u64() {
                        s.turn_timeout_secs = x.clamp(60, 4 * 3600);
                    }
                }
                "verify_timeout_secs" => {
                    if let Some(x) = v.as_u64() {
                        s.verify_timeout_secs = x.clamp(10, 3600);
                    }
                }
                "transcript_budget_chars" => {
                    if let Some(x) = v.as_u64() {
                        s.transcript_budget_chars = (x as usize).clamp(2_000, 200_000);
                    }
                }
                "default_models" => {
                    if let Some(m) = v.as_object() {
                        for (pid, model) in m {
                            match opt_string(model) {
                                Some(model) => {
                                    s.default_models.insert(pid.clone(), model);
                                }
                                None => {
                                    s.default_models.remove(pid);
                                }
                            }
                        }
                    }
                }
                "auto_reroute" => {
                    if let Some(x) = v.as_bool() {
                        s.auto_reroute = x;
                    }
                }
                "context_pack" => {
                    if let Some(x) = v.as_bool() {
                        s.context_pack = x;
                    }
                }
                "isolate_cli_config" => {
                    if let Some(x) = v.as_bool() {
                        s.isolate_cli_config = x;
                    }
                }
                "digest" => {
                    if let Ok(d) = serde_json::from_value::<agora_core::digest::DigestSettings>(v.clone()) {
                        s.digest = d;
                    }
                }
                "clear_unsupported" => {
                    if v.as_bool() == Some(true) {
                        s.unsupported_models.clear();
                    }
                }
                _ => {}
            }
        }
    })?;
    Ok(get_settings(State(st)).await)
}

async fn pairing_info(State(st): State<AppState>) -> Json<Value> {
    let (code, expires_at) = st.pairing.current();
    let s = st.engine.settings();
    let lan_port = st.lan_port().await;
    let lan_candidates = auth::lan_candidates().await;
    let selected_lan = auth::select_lan_candidate(&lan_candidates, auth::lan_ip());
    let ts = auth::tailscale_info().await;
    // The real HTTPS mapping to this Agora, if any (v0.4 guessed :443 — see auth.rs).
    let serve_url = if ts.is_some() { auth::tailscale_serve_url(st.port).await } else { None };
    Json(json!({
        "code": code,
        "expires_at": expires_at,
        "port": st.port,
        "lan_enabled": s.lan_enabled,
        "lan_running": lan_port.is_some(),
        "lan_port": s.lan_port,
        "lan_url": selected_lan.as_ref().zip(lan_port).map(|(candidate, p)| format!("http://{}:{p}/", candidate.ip)),
        "lan_candidates": lan_candidates,
        "tailscale": ts.map(|(ip, dns)| json!({
            "ip": ip,
            "dns": dns,
            "serve_ready": serve_url.is_some(),
            "https_url": serve_url,
        })),
        "devices": s.devices.iter().map(|d| json!({ "id": d.id, "name": d.name, "created_at": d.created_at, "last_seen": d.last_seen })).collect::<Vec<_>>(),
    }))
}

/// Local-only (under `/api/network`): add an HTTPS `tailscale serve` mapping to this Agora on a
/// free port. Never replaces an existing mapping.
async fn enable_tailscale(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    auth::enable_tailscale_serve(st.port).await.map_err(ApiError::bad)?;
    Ok(pairing_info(State(st)).await)
}

async fn pairing_rotate(State(st): State<AppState>) -> Json<Value> {
    st.pairing.rotate();
    pairing_info(State(st)).await
}

async fn revoke_device(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    st.engine.update_settings(|s| s.devices.retain(|d| d.id != id))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NetworkBody {
    lan_enabled: bool,
}

async fn set_network(State(st): State<AppState>, Json(b): Json<NetworkBody>) -> Result<Json<Value>, ApiError> {
    st.engine.update_settings(|s| s.lan_enabled = b.lan_enabled)?;
    if b.lan_enabled {
        st.start_lan().await.map_err(|e| ApiError::bad(format!("無法開啟區網連線埠：{e}")))?;
    } else {
        st.stop_lan().await;
    }
    Ok(pairing_info(State(st)).await)
}

// ================================================================ static

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "apng" => "image/apng",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "webm" => "video/webm",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

async fn static_handler(uri: Uri) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.starts_with("api/") {
        return ApiError::not_found("沒有這個 API").into_response();
    }
    if path.is_empty() {
        path = "index.html".into();
    } else if path == "widget" {
        path = "widget.html".into();
    }
    match Assets::get(&path) {
        Some(file) => {
            let ct = content_type(&path);
            let mut resp = (StatusCode::OK, [(header::CONTENT_TYPE, ct)], file.data.into_owned()).into_response();
            let h = resp.headers_mut();
            let no_cache = ct.starts_with("text/html") || path == "sw.js" || ct.starts_with("text/javascript") || ct.starts_with("text/css");
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static(if no_cache { "no-cache" } else { "max-age=86400" }));
            if ct.starts_with("text/html") {
                h.insert("content-security-policy", HeaderValue::from_static(CSP));
            }
            resp
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Status values the UI treats as finished (exported for the desktop shell).
pub fn is_finished(s: Status) -> bool {
    !matches!(s, Status::Running)
}
