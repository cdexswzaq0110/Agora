//! v0.5 專案團隊 endpoints: the roster, workflows, starting stages and tasks, export.
//! Approval, revisions and section comments of a stage go through the session's own
//! `/api/sessions/{id}/decide` — a stage is an ordinary plan-only workshop session.

use super::*;
use agora_core::engine::{NewWorkflow, OverCap};
use agora_core::team::{self, Depth, ProjectKind, StageKind};

/// Roster, stages and call estimates for the idea composer.
pub(crate) async fn team_info() -> Json<Value> {
    let est = |d: Depth, k: ProjectKind| json!({ "calls": team::total_calls(d, k), "cap": team::workflow_cap(d, k) });
    // Each depth also carries its per-stage call plans, so the home page can show who plays
    // which role at every stage before you start.
    let plans = |d: Depth, k: ProjectKind| StageKind::ALL.iter().map(|s| team::stage_plan(*s, d, k)).collect::<Vec<_>>();
    let depth = |d: Depth| {
        json!({
            "label": d.label(),
            "general": est(d, ProjectKind::General),
            "ai": est(d, ProjectKind::Ai),
            "plans": { "general": plans(d, ProjectKind::General), "ai": plans(d, ProjectKind::Ai) },
        })
    };
    Json(json!({
        "roles": team::ROLES.iter().map(|r| json!({
            "id": r.id, "name": r.name, "title": r.title, "group": r.group, "group_label": r.group.label(), "duty": r.duty,
        })).collect::<Vec<_>>(),
        "stages": StageKind::ALL.iter().map(|k| json!({ "kind": k, "label": k.label() })).collect::<Vec<_>>(),
        "depths": { "quick": depth(Depth::Quick), "standard": depth(Depth::Standard), "full": depth(Depth::Full) },
        "max_idea_chars": team::MAX_IDEA_CHARS,
    }))
}

#[derive(Deserialize)]
pub(crate) struct WorkflowQuery {
    #[serde(default)]
    project_id: Option<String>,
}

pub(crate) async fn list_workflows(State(st): State<AppState>, Query(q): Query<WorkflowQuery>) -> Json<Value> {
    Json(json!(st.engine.workflows(q.project_id.as_deref()).await))
}

pub(crate) async fn create_workflow(State(st): State<AppState>, Json(b): Json<NewWorkflow>) -> ApiResult<Value> {
    Ok(Json(json!(st.engine.create_workflow(b).await?)))
}

fn known(st: &AppState, id: &str) -> Result<(), ApiError> {
    match st.engine.store().load_workflow(id) {
        Some(_) => Ok(()),
        None => Err(ApiError::not_found("找不到工作流")),
    }
}

pub(crate) async fn get_workflow(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    known(&st, &id)?;
    Ok(Json(json!(st.engine.workflow(&id).await?)))
}

#[derive(Deserialize, Default)]
struct StageStartBody {
    /// You confirmed going past the workflow's call ceiling.
    #[serde(default)]
    raise_cap: bool,
}

/// `{kind}` is the stage name (`discovery`, `feasibility`, `product`, `design`, `tasks`).
/// Returns the stage's session (the existing one while it runs or waits for you). Passing the
/// workflow's call ceiling is a 409 until the body says `{"raise_cap": true}`.
pub(crate) async fn start_stage(State(st): State<AppState>, Path((id, kind)): Path<(String, String)>, body: axum::body::Bytes) -> ApiResult<Value> {
    known(&st, &id)?;
    let kind: StageKind = serde_json::from_value(json!(kind)).map_err(|_| ApiError::not_found("沒有這個階段"))?;
    let b: StageStartBody = if body.iter().all(u8::is_ascii_whitespace) { StageStartBody::default() } else { serde_json::from_slice(&body).map_err(|e| ApiError::bad(e.to_string()))? };
    match st.engine.start_stage(&id, kind, b.raise_cap).await {
        Ok(rec) => Ok(Json(json!(rec))),
        Err(e) if e.downcast_ref::<OverCap>().is_some() => Err(ApiError(StatusCode::CONFLICT, e.to_string())),
        Err(e) => Err(e.into()),
    }
}

pub(crate) async fn start_task(State(st): State<AppState>, Path((id, task)): Path<(String, String)>) -> ApiResult<Value> {
    known(&st, &id)?;
    Ok(Json(json!(st.engine.start_task(&id, &task).await?)))
}

/// Writes into the project folder, so only from this computer (like the ADR export).
pub(crate) async fn export_workflow(State(st): State<AppState>, Path(id): Path<String>, axum::Extension(c): axum::Extension<Caller>) -> ApiResult<Value> {
    if !c.local {
        return Err(ApiError(StatusCode::FORBIDDEN, "寫入專案檔案只能在電腦本機操作".into()));
    }
    known(&st, &id)?;
    let paths = st.engine.export_workflow(&id).await?;
    Ok(Json(json!({ "paths": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() })))
}

/// v0.6: the vendored Serendipity skill bundle — version, verification, where each skill is used,
/// and how often it was really sent.
pub(crate) async fn list_skills(State(st): State<AppState>) -> Json<Value> {
    let reg = agora_core::skills::registry();
    let usage = st.engine.skill_usage().await;
    Json(json!({
        "bundle_version": reg.bundle_version,
        "source_repo": reg.source_repo,
        "source_commit": reg.source_commit,
        "ok": reg.ok,
        "problem": reg.problem,
        "skills": reg.skills.iter().map(|s| {
            let u = usage.get(&s.name).cloned().filter(|_| s.tier == "core").unwrap_or_default();
            json!({
                "name": s.name, "description": s.description, "tier": s.tier, "sha256": s.sha256,
                "ok": s.ok, "problem": s.problem, "references": s.references.len(),
                "used_by": if s.tier == "core" { agora_core::skills::used_by(&s.name) } else { vec![] },
                "injected": u.injected, "truncated": u.truncated,
                "review_accepted": u.review_accepted, "review_rejected": u.review_rejected,
            })
        }).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
pub(crate) struct ProveBody {
    provider: String,
}

/// Spends one small CLI call: proves (or not) that this provider's read-only reviewer cannot write.
pub(crate) async fn prove_readonly(State(st): State<AppState>, axum::Extension(c): axum::Extension<Caller>, Json(b): Json<ProveBody>) -> ApiResult<Value> {
    if !c.local {
        return Err(ApiError(StatusCode::FORBIDDEN, "唯讀證明只能在電腦本機執行".into()));
    }
    Ok(Json(json!(st.engine.prove_readonly(&b.provider).await?)))
}
