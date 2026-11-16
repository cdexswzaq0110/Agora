//! v0.2 endpoints: approval gate, workspace (diff/apply/discard), exports, templates,
//! experiments, daily digest, schedules, usage history, pairing QR.

use super::*;
use axum::http::HeaderName;

pub(crate) async fn decide(State(st): State<AppState>, Path(id): Path<String>, Json(d): Json<GateDecision>) -> ApiResult<Value> {
    st.engine.decide(&id, d).await?;
    Ok(Json(json!({ "ok": true })))
}

pub(crate) async fn session_diff(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.workspace_diff(&id).await?)))
}

pub(crate) async fn workspace_apply(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.apply_workspace(&id).await?)))
}

pub(crate) async fn workspace_discard(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.discard_workspace(&id).await?)))
}

pub(crate) async fn export_md(State(st): State<AppState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let (name, body) = st.engine.export_markdown(&id).await?;
    let disposition = format!("attachment; filename=\"{name}\"");
    let mut resp = (StatusCode::OK, [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], body).into_response();
    resp.headers_mut().insert(
        HeaderName::from_static("content-disposition"),
        HeaderValue::from_str(&disposition).map_err(|e| ApiError::bad(e.to_string()))?,
    );
    Ok(resp)
}

#[derive(Deserialize)]
pub(crate) struct ExportBody {
    kind: String,
}

pub(crate) async fn export_file(
    State(st): State<AppState>,
    Path(id): Path<String>,
    axum::Extension(c): axum::Extension<Caller>,
    Json(b): Json<ExportBody>,
) -> ApiResult<Value> {
    if !c.local {
        return Err(ApiError(StatusCode::FORBIDDEN, "寫入專案檔案只能在電腦本機操作".into()));
    }
    let path = st.engine.export_file(&id, &b.kind).await?;
    Ok(Json(json!({ "path": path.display().to_string() })))
}

pub(crate) async fn list_templates(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.templates()))
}

pub(crate) async fn save_template(State(st): State<AppState>, Json(t): Json<Template>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.save_template(t)?)))
}

pub(crate) async fn delete_template(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    st.engine.delete_template(&id)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ExperimentBody {
    spec: SessionSpec,
    modes: Vec<Mode>,
}

pub(crate) async fn start_experiment(State(st): State<AppState>, Json(b): Json<ExperimentBody>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.start_experiment(b.spec, b.modes).await?)))
}

pub(crate) async fn list_experiments(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.experiments()))
}

pub(crate) async fn get_experiment(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    st.engine.experiment(&id).await.map(|v| Json(json!(v))).ok_or_else(|| ApiError::not_found("找不到實驗"))
}

pub(crate) async fn list_digests(State(st): State<AppState>) -> Json<Value> {
    let dates = st.engine.store().digest_dates();
    let s = st.engine.settings().digest;
    Json(json!({ "dates": dates, "running": st.engine.digest_running(), "enabled": s.enabled, "time": s.time }))
}

pub(crate) async fn get_digest(State(st): State<AppState>, Path(date): Path<String>) -> ApiResult<Value> {
    st.engine.digest(&date).map(|d| Json(json!(d))).ok_or_else(|| ApiError::not_found("這一天還沒有技術情報"))
}

pub(crate) async fn digest_state(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.digest_state()))
}

#[derive(Deserialize)]
pub(crate) struct ReadBody {
    generated_at: u64,
}

/// Called by the page only after that exact version was loaded and painted.
pub(crate) async fn mark_digest_read(State(st): State<AppState>, Path(date): Path<String>, Json(b): Json<ReadBody>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.mark_digest_read(&date, b.generated_at)?)))
}

pub(crate) async fn run_digest(State(st): State<AppState>) -> ApiResult<Value> {
    if st.engine.digest_running() {
        return Err(ApiError::bad("今日技術情報正在整理中"));
    }
    let e = st.engine.clone();
    tokio::spawn(async move {
        let _ = e.run_digest().await;
    });
    Ok(Json(json!({ "started": true })))
}

pub(crate) async fn list_schedules(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.settings().schedules))
}

pub(crate) async fn save_schedule(State(st): State<AppState>, Json(s): Json<Schedule>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.upsert_schedule(s)?)))
}

pub(crate) async fn delete_schedule(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    st.engine.delete_schedule(&id)?;
    Ok(Json(json!({ "ok": true })))
}

pub(crate) async fn run_schedule(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let today = agora_core::scheduler::local_date();
    match st.engine.run_schedule(&id, &today).await {
        Some(r) => Ok(Json(json!(r?))),
        None => Err(ApiError::not_found("找不到排程")),
    }
}

#[derive(Deserialize)]
pub(crate) struct DaysQuery {
    days: Option<u64>,
}

pub(crate) async fn usage_history(State(st): State<AppState>, Query(q): Query<DaysQuery>) -> Json<Value> {
    Json(json!(st.engine.quota_history(q.days.unwrap_or(7).clamp(1, 90))))
}

pub(crate) async fn usage_stats(State(st): State<AppState>, Query(q): Query<DaysQuery>) -> Json<Value> {
    Json(json!(st.engine.usage_stats(q.days.unwrap_or(30).clamp(1, 365)).await))
}

#[derive(Deserialize)]
pub(crate) struct QrQuery {
    target: Option<String>,
    ip: Option<String>,
}

/// `ip` may only name one of this machine's listed LAN addresses — never an arbitrary string
/// in the QR code.
fn validated_qr_ip(requested: Option<&str>, candidates: &[auth::LanCandidate]) -> Result<Option<std::net::Ipv4Addr>, ApiError> {
    let Some(requested) = requested else { return Ok(None) };
    candidates
        .iter()
        .find(|candidate| candidate.ip.to_string() == requested)
        .map(|candidate| Some(candidate.ip))
        .ok_or_else(|| ApiError::bad("指定的 IP 不在可用區網網卡清單中"))
}

/// QR code for pairing a phone: the URL carries the one-time code in the fragment (never sent
/// to any server), and the pairing page pre-fills it.
pub(crate) async fn pairing_qr(State(st): State<AppState>, Query(q): Query<QrQuery>) -> Result<Response, ApiError> {
    let (code, _) = st.pairing.current();
    let candidates = auth::lan_candidates().await;
    let requested_ip = validated_qr_ip(q.ip.as_deref(), &candidates)?;
    let selected_ip = requested_ip.or_else(|| auth::select_lan_candidate(&candidates, auth::lan_ip()).map(|candidate| candidate.ip));
    let lan = selected_ip.zip(st.lan_port().await).map(|(ip, p)| format!("http://{ip}:{p}/"));
    let ts = auth::tailscale_serve_url(st.port).await;
    let base = match q.target.as_deref() {
        Some("lan") => lan,
        Some("tailscale") => ts,
        _ => lan.or(ts),
    }
    .ok_or_else(|| ApiError::bad("沒有可用的手機網址：請先開啟區網連線或設定 Tailscale"))?;
    let url = format!("{base}#pair={code}");
    let svg = qrcode::QrCode::new(url.as_bytes())
        .map_err(|e| ApiError::bad(e.to_string()))?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(qrcode::render::svg::Color("#111111"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    let mut resp = (StatusCode::OK, [(header::CONTENT_TYPE, "image/svg+xml")], svg).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut().insert(HeaderName::from_static("x-agora-url"), HeaderValue::from_str(&base).map_err(|e| ApiError::bad(e.to_string()))?);
    Ok(resp)
}

#[cfg(test)]
mod pairing_qr_tests {
    use super::*;

    fn candidates() -> Vec<auth::LanCandidate> {
        vec![auth::LanCandidate { ip: "192.168.1.8".parse().unwrap(), name: "Wi-Fi".into(), virtual_adapter: false }]
    }

    #[test]
    fn qr_ip_must_be_a_listed_candidate() {
        assert!(matches!(validated_qr_ip(Some("192.168.1.8"), &candidates()), Ok(Some(ip)) if ip == "192.168.1.8".parse::<std::net::Ipv4Addr>().unwrap()));
        assert!(validated_qr_ip(Some("10.0.0.9"), &candidates()).is_err());
        assert!(validated_qr_ip(Some("evil.example"), &candidates()).is_err());
        assert!(matches!(validated_qr_ip(None, &candidates()), Ok(None)));
    }
}
