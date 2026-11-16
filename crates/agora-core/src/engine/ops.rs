//! Engine operations outside a running session: approval decisions (also after a restart),
//! workspace diff/apply/discard, exports, templates, mode-comparison experiments, the daily
//! digest, scheduled sessions and usage statistics.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::runner::{RunCtl, Runner};
use super::{Engine, GateDecision};

pub const MAX_PLAN_COMMENTS: usize = 20;

/// Section comments are user data headed into a prompt: bounded, anchored to a section of the
/// *current* revision, and free of control characters (newlines and tabs are fine).
pub fn validate_revision(plan: &Plan, feedback: &str, comments: &[PlanComment], plan_revision: Option<u32>) -> Result<()> {
    if feedback.trim().is_empty() && comments.iter().all(|c| c.text.trim().is_empty()) {
        bail!("請寫下修改意見，或至少一則段落留言");
    }
    if feedback.chars().count() > 4000 {
        bail!("修改意見太長（上限 4,000 字）");
    }
    if comments.is_empty() {
        return Ok(());
    }
    if comments.len() > MAX_PLAN_COMMENTS {
        bail!("段落留言最多 {MAX_PLAN_COMMENTS} 則");
    }
    if plan_revision != Some(plan.revision) {
        bail!("方案已經更新（現在是第 {} 版），這些留言是針對舊版本寫的；請確認後重新送出", plan.revision + 1);
    }
    let bad = |s: &str| s.chars().any(|c| c.is_control() && c != '\n' && c != '\t');
    for c in comments {
        if c.text.trim().is_empty() {
            bail!("有一則段落留言是空的");
        }
        if c.text.chars().count() > 1000 || c.quote.chars().count() > 300 || c.heading.chars().count() > 120 {
            bail!("段落留言太長（留言 1,000 字、引文 300 字、標題 120 字）");
        }
        if bad(&c.text) || bad(&c.quote) || bad(&c.heading) {
            bail!("段落留言含有不允許的控制字元");
        }
        if !plan.blocks.iter().any(|b| b.id == c.block_id) {
            bail!("找不到留言對應的段落（{}）；方案可能已經更新", c.block_id);
        }
    }
    Ok(())
}

/// `GET /api/digests/state` — drives the 情報 dot in the nav.
#[derive(Clone, Debug, Serialize)]
pub struct DigestState {
    pub latest: Option<String>,
    pub generated_at: Option<u64>,
    pub unread: bool,
    pub running: bool,
}
use crate::digest::{self, rank, Digest};
use crate::event::Event;
use crate::export;
use crate::git;
use crate::history::{self, QuotaSample, UsageStats};
use crate::model::*;
use crate::provider::{Provider, ProviderEvent, TurnRequest};
use crate::scheduler::{self, Schedule};
use crate::templates::{self, Template};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experiment {
    pub id: String,
    pub title: String,
    pub project_id: String,
    pub prompt: String,
    pub modes: Vec<Mode>,
    pub session_ids: Vec<String>,
    pub errors: Vec<String>,
    pub created_at: u64,
    pub done: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExperimentView {
    pub experiment: Experiment,
    pub sessions: Vec<SessionSummary>,
}

impl Engine {
    // ---------------------------------------------------------------- approval gate

    /// Deliver a decision to a workshop waiting at the gate. Works for a live runner and for a
    /// session left waiting across an Agora restart (a new runner resumes from the gate).
    pub async fn decide(self: &Arc<Self>, id: &str, decision: GateDecision) -> Result<()> {
        if let GateDecision::Revise { feedback, comments, plan_revision } = &decision {
            let plan = match self.running.lock().await.get(id).cloned() {
                Some(ctl) => ctl.record.lock().await.plan.clone(),
                None => self.store.load_session(id).and_then(|r| r.plan),
            }
            .ok_or_else(|| anyhow!("這個議題沒有方案"))?;
            validate_revision(&plan, feedback, comments, *plan_revision)?;
        }
        if let Some(ctl) = self.running.lock().await.get(id).cloned() {
            let tx = ctl.gate.lock().await.take().ok_or_else(|| anyhow!("這個議題目前不在等待核准"))?;
            tx.send(decision).map_err(|_| anyhow!("議題已結束"))?;
            return Ok(());
        }
        let rec = self.store.load_session(id).ok_or_else(|| anyhow!("找不到議題"))?;
        if rec.status != Status::AwaitingApproval {
            bail!("這個議題目前不在等待核准");
        }
        let project = self.project(&rec.spec.project_id).ok_or_else(|| anyhow!("找不到專案"))?;
        let used = rec.turns.iter().filter(|t| !t.seeded).count() as u32;
        let ctl = Arc::new(RunCtl::new(rec.clone()));
        self.running.lock().await.insert(rec.id.clone(), ctl.clone());
        let runner = Arc::new(Runner {
            engine: self.clone(),
            ctl,
            session_id: rec.id.clone(),
            workdir: std::sync::Mutex::new(project.path.clone().into()),
            project,
            spec: rec.spec.clone(),
            settings: self.settings(),
            turns_used: AtomicU32::new(used),
            max_turns: Self::turn_budget(&rec.spec),
            round_offset: 0,
            snapshot: None,
            resume: std::sync::Mutex::new(Some(decision)),
        });
        self.emit(Event::Status { session_id: rec.id.clone(), title: rec.title.clone(), status: Status::Running, error: None });
        tokio::spawn(runner.run());
        Ok(())
    }

    // ---------------------------------------------------------------- workspace

    fn finished_record(&self, id: &str) -> Result<SessionRecord> {
        let r = self.store.load_session(id).ok_or_else(|| anyhow!("找不到議題"))?;
        if r.status.is_live() {
            bail!("議題還在進行中，結束後才能套用或丟棄");
        }
        Ok(r)
    }

    fn worktree_parts(ws: &Workspace) -> Result<(PathBuf, String, String)> {
        if ws.kind != WorkspaceKind::Worktree {
            bail!("這個議題是直接修改專案資料夾，沒有隔離工作區");
        }
        if ws.state != WorkspaceState::Active {
            bail!("隔離工作區已經{}", if ws.state == WorkspaceState::Applied { "套用" } else { "丟棄" });
        }
        Ok((
            PathBuf::from(ws.root.clone().ok_or_else(|| anyhow!("缺少 worktree 路徑"))?),
            ws.base.clone().ok_or_else(|| anyhow!("缺少基準 commit"))?,
            ws.branch.clone().ok_or_else(|| anyhow!("缺少分支名稱"))?,
        ))
    }

    pub async fn workspace_diff(&self, id: &str) -> Result<git::DiffSummary> {
        let r = self.session(id).await.ok_or_else(|| anyhow!("找不到議題"))?;
        let ws = r.workspace.ok_or_else(|| anyhow!("這個議題沒有修改檔案"))?;
        match ws.kind {
            WorkspaceKind::Worktree if ws.state == WorkspaceState::Active => {
                let (root, base, _) = Self::worktree_parts(&ws)?;
                git::diff(&root, &base).await
            }
            WorkspaceKind::Worktree => bail!("隔離工作區已經{}", if ws.state == WorkspaceState::Applied { "套用到專案" } else { "丟棄" }),
            WorkspaceKind::Direct => {
                let p = PathBuf::from(&ws.path);
                if !git::is_repo(&p).await {
                    bail!("專案不是 git repo，無法計算 diff");
                }
                // Changes made directly in the project, against its HEAD (may include the
                // user's own uncommitted edits).
                let numstat = git::git(&p, &["diff", "--numstat", "HEAD"]).await?;
                let files = git::parse_numstat(&numstat);
                let patch = git::git(&p, &["diff", "HEAD"]).await?;
                Ok(git::DiffSummary {
                    added: files.iter().filter_map(|f| f.added).sum(),
                    removed: files.iter().filter_map(|f| f.removed).sum(),
                    files,
                    truncated: patch.len() > git::MAX_PATCH_BYTES,
                    patch: patch.chars().take(git::MAX_PATCH_BYTES).collect(),
                })
            }
        }
    }

    async fn update_workspace(&self, mut r: SessionRecord, state: WorkspaceState, note: String) -> Result<SessionRecord> {
        if let Some(ws) = r.workspace.as_mut() {
            ws.state = state;
            ws.message = Some(note.clone());
        }
        r.notes.push(Note { at: now_ms(), level: NoteLevel::Info, text: note });
        r.updated_at = now_ms();
        self.store.save_session(&r)?;
        if let Some(ws) = r.workspace.clone() {
            self.emit(Event::Workspace { session_id: r.id.clone(), workspace: ws });
        }
        Ok(r)
    }

    /// Apply the worktree's changes to the project (uncommitted), then remove the worktree.
    pub async fn apply_workspace(&self, id: &str) -> Result<SessionRecord> {
        let r = self.finished_record(id)?;
        let ws = r.workspace.clone().ok_or_else(|| anyhow!("這個議題沒有修改檔案"))?;
        let (root, base, branch) = Self::worktree_parts(&ws)?;
        let project = PathBuf::from(&self.project(&r.spec.project_id).ok_or_else(|| anyhow!("找不到專案"))?.path);
        let n = git::apply(&project, &root, &base).await?;
        git::remove(&project, &root, &branch).await?;
        self.update_workspace(r, WorkspaceState::Applied, format!("已把 {n} 個檔案的變更套用到專案（尚未 commit，請檢查後自行 commit）；隔離工作區已移除。")).await
    }

    pub async fn discard_workspace(&self, id: &str) -> Result<SessionRecord> {
        let r = self.finished_record(id)?;
        let ws = r.workspace.clone().ok_or_else(|| anyhow!("這個議題沒有修改檔案"))?;
        let (root, _, branch) = Self::worktree_parts(&ws)?;
        let project = PathBuf::from(&self.project(&r.spec.project_id).ok_or_else(|| anyhow!("找不到專案"))?.path);
        git::remove(&project, &root, &branch).await?;
        self.update_workspace(r, WorkspaceState::Discarded, "已丟棄隔離工作區，專案沒有任何改動。".into()).await
    }

    // ---------------------------------------------------------------- exports

    pub async fn export_markdown(&self, id: &str) -> Result<(String, String)> {
        let r = self.session(id).await.ok_or_else(|| anyhow!("找不到議題"))?;
        let p = self.project(&r.spec.project_id);
        let name = format!("agora-{}-{}.md", format_date_compact(r.created_at), export::slug(&r.title, &r.id));
        Ok((name, export::session_markdown(&r, p.as_ref())))
    }

    /// Write an ADR or a lesson draft into the session's project. Returns the file path.
    pub async fn export_file(&self, id: &str, kind: &str) -> Result<PathBuf> {
        let r = self.session(id).await.ok_or_else(|| anyhow!("找不到議題"))?;
        if r.status.is_live() {
            bail!("議題結束後才能匯出");
        }
        let project = self.project(&r.spec.project_id).ok_or_else(|| anyhow!("找不到專案"))?;
        let dir = Path::new(&project.path);
        match kind {
            "adr" => export::write_adr(dir, &r),
            "lesson" => export::write_lesson(dir, &r),
            _ => bail!("未知的匯出類型：{kind}"),
        }
    }

    // ---------------------------------------------------------------- templates

    pub fn templates(&self) -> Vec<Template> {
        let mut v = templates::builtins();
        v.extend(self.settings().templates);
        v
    }

    pub fn save_template(&self, mut t: Template) -> Result<Template> {
        if t.name.trim().is_empty() {
            bail!("請輸入範本名稱");
        }
        t.builtin = false;
        if t.id.trim().is_empty() || templates::builtins().iter().any(|b| b.id == t.id) {
            t.id = new_id("t");
        }
        let saved = t.clone();
        self.update_settings(|s| {
            s.templates.retain(|x| x.id != t.id);
            s.templates.push(t);
        })?;
        Ok(saved)
    }

    pub fn delete_template(&self, id: &str) -> Result<()> {
        self.update_settings(|s| s.templates.retain(|x| x.id != id))?;
        Ok(())
    }

    // ---------------------------------------------------------------- experiments

    /// Run the same question through several modes, one after another (to keep quota use
    /// predictable), read-only, and compare the outcomes.
    pub async fn start_experiment(self: &Arc<Self>, base: SessionSpec, modes: Vec<Mode>) -> Result<Experiment> {
        let mut modes = modes;
        modes.dedup();
        if modes.len() < 2 || modes.len() > 5 {
            bail!("請選 2 到 5 種模式比較");
        }
        if base.prompt.trim().is_empty() {
            bail!("請輸入議題內容");
        }
        let exp = Experiment {
            id: new_id("x"),
            title: crate::prompts::headline(&base.prompt, 40),
            project_id: base.project_id.clone(),
            prompt: base.prompt.clone(),
            modes: modes.clone(),
            session_ids: vec![],
            errors: vec![],
            created_at: now_ms(),
            done: false,
        };
        self.store.save_experiment(&exp)?;
        let me = self.clone();
        let mut state = exp.clone();
        tokio::spawn(async move {
            for mode in modes {
                let mut spec = base.clone();
                spec.mode = mode;
                spec.group_id = Some(state.id.clone());
                spec.access = Access::ReadOnly;
                spec.title = Some(format!("{}｜{}", state.title, mode.label()));
                if mode == Mode::Workshop {
                    spec.plan_only = true;
                    spec.auto_approve = true;
                }
                match me.start(spec).await {
                    Ok(rec) => {
                        state.session_ids.push(rec.id.clone());
                        let _ = me.store.save_experiment(&state);
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            match me.session(&rec.id).await {
                                Some(r) if r.status.is_live() => continue,
                                _ => break,
                            }
                        }
                    }
                    Err(e) => state.errors.push(format!("{}：{e:#}", mode.label())),
                }
            }
            state.done = true;
            let _ = me.store.save_experiment(&state);
        });
        Ok(exp)
    }

    pub async fn experiment(&self, id: &str) -> Option<ExperimentView> {
        let experiment = self.store.load_experiment(id)?;
        let mut sessions = Vec::new();
        for sid in &experiment.session_ids {
            if let Some(r) = self.session(sid).await {
                sessions.push(SessionSummary::from(&r));
            }
        }
        Some(ExperimentView { experiment, sessions })
    }

    pub fn experiments(&self) -> Vec<Experiment> {
        self.store.experiments()
    }

    // ---------------------------------------------------------------- daily digest

    /// The scheduler may try again: nothing running and no attempt in the last hour.
    pub fn digest_may_retry(&self) -> bool {
        if self.digest_running.load(Ordering::SeqCst) {
            return false;
        }
        self.digest_last_attempt.lock().unwrap().map_or(true, |t| now_ms().saturating_sub(t) > 3_600_000)
    }

    pub fn digest_running(&self) -> bool {
        self.digest_running.load(Ordering::SeqCst)
    }

    pub async fn run_digest(self: &Arc<Self>) -> Result<Digest> {
        if self.digest_running.swap(true, Ordering::SeqCst) {
            bail!("今日技術情報正在整理中");
        }
        *self.digest_last_attempt.lock().unwrap() = Some(now_ms());
        let date = scheduler::local_date();
        self.emit(Event::Digest { status: "running".into(), date: date.clone(), error: None });
        let res = self.build_digest(&date).await;
        self.digest_running.store(false, Ordering::SeqCst);
        match &res {
            Ok(_) => self.emit(Event::Digest { status: "done".into(), date, error: None }),
            Err(e) => self.emit(Event::Digest { status: "failed".into(), date, error: Some(format!("{e:#}")) }),
        }
        res
    }

    async fn build_digest(&self, date: &str) -> Result<Digest> {
        let ds = self.settings().digest;
        let (items, sources) = digest::sources::collect(&ds).await;
        self.compose_digest(date, items, sources).await
    }

    /// Rank, summarize (CLI model, or deterministic fallback) and store a digest from already
    /// fetched items. Split from the network step so it can be tested offline.
    pub async fn compose_digest(&self, date: &str, items: Vec<digest::DigestItem>, sources: Vec<digest::SourceStatus>) -> Result<Digest> {
        let ds = self.settings().digest;
        if items.is_empty() {
            let why: Vec<String> = sources.iter().filter_map(|s| s.error.as_ref().map(|e| format!("{}：{e}", s.label))).collect();
            bail!("所有來源都沒有抓到資料{}", if why.is_empty() { String::new() } else { format!("（{}）", why.join("；")) });
        }
        let selected = rank::select(items, &ds.keywords, ds.max_items.clamp(10, 80));
        let mut d = Digest { date: date.into(), generated_at: now_ms(), items: selected, sources, ..Default::default() };
        // 和昨天相比 — from stored digests only; no extra model call.
        digest::trend::annotate(&mut d, &self.digest_history(date), &ds.keywords);
        if ds.use_llm {
            match self.summarize_digest(&d, &ds).await {
                Ok((text, who, usage)) => {
                    d.markdown = text;
                    d.llm_ok = true;
                    d.summarizer = Some(who);
                    d.usage = usage;
                }
                Err(e) => d.llm_error = Some(e),
            }
        }
        if !d.llm_ok {
            let reason = d.llm_error.clone().unwrap_or_else(|| "已在設定關閉模型整理".into());
            d.markdown = digest::fallback_markdown(&d.items, Some(&reason), digest::trend::summary_markdown(&d).as_deref());
        }
        self.store.save_digest(&d)?;
        Ok(d)
    }

    fn digest_provider(&self, preferred: Option<&str>) -> Option<Arc<dyn Provider>> {
        if let Some(p) = preferred.and_then(|id| self.provider(id)) {
            return Some(p);
        }
        let ready = |p: &Arc<dyn Provider>| self.health_of(p.id()).is_some_and(|h| h.ok) && self.quota_block(p.id()).is_none();
        let all = self.providers();
        all.iter().filter(|p| !p.is_mock()).find(|p| ready(p)).or_else(|| all.iter().filter(|p| p.is_mock()).find(|p| ready(p))).cloned()
    }

    async fn summarize_digest(&self, d: &Digest, ds: &digest::DigestSettings) -> std::result::Result<(String, String, Option<TokenUsage>), String> {
        if self.health.read().unwrap().is_empty() {
            self.refresh_health().await;
        }
        let provider = self.digest_provider(ds.provider.as_deref()).ok_or("沒有就緒、且額度足夠的 CLI 模型")?;
        if let Some(reason) = self.quota_block(provider.id()) {
            return Err(format!("額度保護：{reason}"));
        }
        let settings = self.settings();
        let model = ds.model.clone().filter(|m| !m.is_empty()).or_else(|| settings.default_models.get(provider.id()).cloned().filter(|m| !m.is_empty()));
        let (tx, mut rx) = mpsc::unbounded_channel();
        // Effort only when the summarizing model lists it (settings may outlive a model change).
        let effort = ds.effort.filter(|e| crate::provider::supported_efforts(&provider.models(), model.as_deref()).contains(e));
        let req = TurnRequest {
            prompt: digest::llm_prompt(&d.items, &ds.keywords, &d.date, d.compared_with.as_deref().map(|p| (p, d.topic_trends.as_slice()))),
            model: model.clone(),
            cwd: self.store.sandbox_dir(),
            access: Access::ReadOnly,
            timeout: Duration::from_secs(15 * 60),
            cancel: CancellationToken::new(),
            effort,
            clean_config: settings.isolate_cli_config,
        };
        let p = provider.clone();
        let handle = tokio::spawn(async move { p.run(req, tx).await });
        while let Some(ev) = rx.recv().await {
            if let ProviderEvent::Quota(q) = ev {
                self.record_quota(q);
            }
        }
        let out = handle.await.map_err(|e| e.to_string())?;
        if out.unsupported_model {
            if let Some(m) = out.rejected_model.clone().or(model.clone()) {
                self.mark_unsupported(provider.id(), &m);
            }
        }
        if !out.ok || out.text.trim().is_empty() {
            return Err(out.error.unwrap_or_else(|| "模型沒有回傳內容".into()));
        }
        Ok((out.text, format!("{} · {}", provider.display_name(), model.unwrap_or_else(|| "預設模型".into())), out.usage))
    }

    pub fn digest(&self, date: &str) -> Option<Digest> {
        let mut d = if date == "latest" { self.store.digest_dates().first().and_then(|d| self.store.load_digest(d)) } else { self.store.load_digest(date) }?;
        // Digests stored before v0.4 have no trends: compute them on read, never rewrite history.
        if d.compared_with.is_none() && d.items.iter().all(|i| i.trend.is_none()) {
            let (history, keywords) = (self.digest_history(&d.date), self.settings().digest.keywords);
            digest::trend::annotate(&mut d, &history, &keywords);
        }
        Some(d)
    }

    /// Latest digest and whether that exact version has been opened (on any device).
    pub fn digest_state(&self) -> DigestState {
        let latest = self.store.digest_dates().into_iter().next();
        let generated_at = latest.as_deref().and_then(|d| self.store.load_digest(d)).map(|d| d.generated_at);
        let reads = self.store.digest_reads();
        let unread = match (&latest, generated_at) {
            (Some(d), Some(g)) => reads.get(d).is_none_or(|r| *r < g),
            _ => false,
        };
        DigestState { latest, generated_at, unread, running: self.digest_running() }
    }

    pub fn mark_digest_read(&self, date: &str, generated_at: u64) -> Result<DigestState> {
        self.store.mark_digest_read(date, generated_at)?;
        Ok(self.digest_state())
    }

    /// Stored digests dated strictly before `date`, newest first, at most the trend window.
    fn digest_history(&self, date: &str) -> Vec<Digest> {
        self.store
            .digest_dates()
            .into_iter()
            .filter(|d| d.as_str() < date)
            .take(digest::trend::WINDOW_DAYS)
            .filter_map(|d| self.store.load_digest(&d))
            .collect()
    }

    // ---------------------------------------------------------------- schedules

    pub fn upsert_schedule(&self, mut s: Schedule) -> Result<Schedule> {
        if s.name.trim().is_empty() {
            bail!("請輸入排程名稱");
        }
        if s.spec.prompt.trim().is_empty() {
            bail!("請輸入排程要執行的議題");
        }
        let (h, m) = scheduler::parse_hm(&s.time);
        s.time = format!("{h:02}:{m:02}");
        s.weekdays.retain(|d| (1..=7).contains(d));
        if s.id.trim().is_empty() {
            s.id = new_id("c");
        }
        let saved = s.clone();
        self.update_settings(|st| {
            st.schedules.retain(|x| x.id != s.id);
            st.schedules.push(s);
        })?;
        Ok(saved)
    }

    pub fn delete_schedule(&self, id: &str) -> Result<()> {
        self.update_settings(|s| s.schedules.retain(|x| x.id != id))?;
        Ok(())
    }

    /// Start a scheduled session now (marks it as run today first, so it never doubles).
    pub async fn run_schedule(self: &Arc<Self>, id: &str, today: &str) -> Option<Result<SessionRecord>> {
        let s = self.settings().schedules.into_iter().find(|s| s.id == id)?;
        let _ = self.update_settings(|st| {
            if let Some(x) = st.schedules.iter_mut().find(|x| x.id == id) {
                x.last_run = Some(today.to_string());
            }
        });
        let mut spec = s.spec.clone();
        spec.title = Some(format!("{}（排程 {today}）", s.name));
        Some(self.start(spec).await)
    }

    // ---------------------------------------------------------------- usage stats

    pub fn quota_history(&self, days: u64) -> Vec<QuotaSample> {
        history::read_since(&self.store.quota_history_path(), now_ms().saturating_sub(days * 86_400_000))
    }

    pub async fn usage_stats(&self, days: u64) -> UsageStats {
        history::stats(&self.all_records().await, now_ms().saturating_sub(days * 86_400_000))
    }
}

fn format_date_compact(ms: u64) -> String {
    super::format_date(ms).replace('-', "")
}
