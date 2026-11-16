//! The engine: owns providers, projects, sessions, quota, the event bus and the scheduler
//! state, and runs the five collaboration modes (see `modes.rs`).
//!
//! Cost is always bounded (Agent Kernel ADR 0002 / CLAUDE.md "a critique mechanism must bound
//! its own cost"): every session has a hard turn budget, revisions are capped, plan revisions
//! are capped, and a quota guard refuses (or reroutes) turns when a subscription window is
//! nearly exhausted. When a bound is hit the session ends with what it has and records why.

mod modes;
mod ops;
mod runner;

pub use ops::Experiment;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::compact;
use crate::event::Event;
use crate::history::{self, QuotaSample};
use crate::model::*;
use crate::prompts;
use crate::provider::{
    claude::ClaudeCli, codex::CodexCli, mock::MockProvider, Health, ModelOption, Provider, ProviderEvent, TurnRequest,
};
use crate::quota::QuotaSnapshot;
use crate::store::{Settings, Store};
use runner::{RunCtl, Runner};

/// Absolute ceiling on CLI calls per session, whatever the spec asks for.
pub const HARD_TURN_CAP: u32 = 24;
pub const MAX_PROMPT_CHARS: usize = 40_000;
/// Human-requested plan revisions per workshop.
pub const MAX_PLAN_REVISIONS: u32 = 2;

#[derive(Debug)]
pub(crate) enum Stop {
    Cancelled,
    Budget(u32),
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Cancelled => write!(f, "已取消"),
            Stop::Budget(n) => write!(f, "已達本議題的發言上限（{n} 次）"),
        }
    }
}
impl std::error::Error for Stop {}

/// The human decision at the workshop approval gate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GateDecision {
    /// Approve (optionally with an edited plan and a verify command).
    Approve {
        #[serde(default)]
        plan: Option<String>,
        #[serde(default)]
        verify_command: Option<String>,
    },
    /// Send the plan back to the synthesizer with feedback and/or section comments. Comments
    /// carry the plan revision they were written against (`plan_revision`).
    Revise {
        #[serde(default)]
        feedback: String,
        #[serde(default)]
        comments: Vec<PlanComment>,
        #[serde(default)]
        plan_revision: Option<u32>,
    },
    /// Keep the plan, do not implement.
    Reject {
        #[serde(default)]
        reason: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub family: Family,
    pub name: String,
    pub mock: bool,
    pub models: Vec<ModelOption>,
    pub default_model: Option<String>,
    pub default_effort: Option<Effort>,
    pub unsupported_models: Vec<String>,
    pub health: Option<Health>,
    pub quota: Option<QuotaSnapshot>,
}

pub struct Engine {
    store: Store,
    settings: RwLock<Settings>,
    providers: RwLock<Vec<Arc<dyn Provider>>>,
    bus: broadcast::Sender<Event>,
    running: Mutex<HashMap<String, Arc<RunCtl>>>,
    quotas: RwLock<HashMap<String, QuotaSnapshot>>,
    health: RwLock<HashMap<String, Health>>,
    last_samples: std::sync::Mutex<HashMap<String, QuotaSample>>,
    digest_running: AtomicBool,
    digest_last_attempt: std::sync::Mutex<Option<u64>>,
}

pub(crate) fn now_secs() -> u64 {
    now_ms() / 1000
}

fn cli_providers(s: &Settings) -> Vec<Arc<dyn Provider>> {
    let mut v: Vec<Arc<dyn Provider>> = vec![
        Arc::new(ClaudeCli::new(s.claude_path.clone())),
        Arc::new(CodexCli::new(s.codex_path.clone())),
    ];
    let demo_env = std::env::var("AGORA_DEMO").is_ok_and(|v| v == "1");
    if s.demo_mode || demo_env {
        v.push(Arc::new(MockProvider::new("mock-claude", Family::Claude)));
        v.push(Arc::new(MockProvider::new("mock-gpt", Family::Gpt)));
    }
    v
}

impl Engine {
    /// Production engine: the Claude and Codex CLI providers (+ mocks in demo mode).
    pub fn new(store: Store) -> Result<Arc<Engine>> {
        let settings = store.settings();
        let providers = cli_providers(&settings);
        Self::assemble(store, settings, providers)
    }

    /// Engine with an explicit provider list (tests).
    pub fn with_providers(store: Store, providers: Vec<Arc<dyn Provider>>) -> Result<Arc<Engine>> {
        let settings = store.settings();
        Self::assemble(store, settings, providers)
    }

    fn assemble(store: Store, settings: Settings, providers: Vec<Arc<dyn Provider>>) -> Result<Arc<Engine>> {
        store.ensure_default_project()?;
        let (bus, _) = broadcast::channel(4096);
        let quotas = store.quota_cache().into_iter().map(|q| (q.provider.clone(), q)).collect();
        let e = Engine {
            store,
            settings: RwLock::new(settings),
            providers: RwLock::new(providers),
            bus,
            running: Mutex::new(HashMap::new()),
            quotas: RwLock::new(quotas),
            health: RwLock::new(HashMap::new()),
            last_samples: std::sync::Mutex::new(HashMap::new()),
            digest_running: AtomicBool::new(false),
            digest_last_attempt: std::sync::Mutex::new(None),
        };
        e.recover_interrupted();
        Ok(Arc::new(e))
    }

    /// Sessions left `running` by a crash/quit are marked interrupted. Sessions waiting at the
    /// approval gate keep waiting — approving them later resumes from the build phase.
    fn recover_interrupted(&self) {
        for mut r in self.store.sessions().into_iter().filter(|r| r.status == Status::Running) {
            r.status = Status::Failed;
            r.error = Some("Agora 結束時這個議題仍在進行，已標記為中斷".into());
            r.updated_at = now_ms();
            let _ = self.store.save_session(&r);
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Apply and persist a settings change. Provider-affecting fields rebuild the providers.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let (new, rebuild) = {
            let mut s = self.settings.write().unwrap();
            let before = (s.claude_path.clone(), s.codex_path.clone(), s.demo_mode);
            f(&mut s);
            s.quota_stop_percent = s.quota_stop_percent.clamp(50.0, 100.0);
            let after = (s.claude_path.clone(), s.codex_path.clone(), s.demo_mode);
            (s.clone(), before != after)
        };
        self.store.save_settings(&new)?;
        if rebuild {
            *self.providers.write().unwrap() = cli_providers(&new);
            self.health.write().unwrap().clear();
        }
        Ok(new)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.bus.subscribe()
    }

    pub(crate) fn emit(&self, ev: Event) {
        let _ = self.bus.send(ev);
    }

    pub fn provider(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.read().unwrap().iter().find(|p| p.id() == id).cloned()
    }

    pub fn providers(&self) -> Vec<Arc<dyn Provider>> {
        self.providers.read().unwrap().clone()
    }

    pub fn health_of(&self, id: &str) -> Option<Health> {
        self.health.read().unwrap().get(id).cloned()
    }

    pub fn providers_info(&self) -> Vec<ProviderInfo> {
        let s = self.settings();
        let health = self.health.read().unwrap();
        let quotas = self.quotas.read().unwrap();
        self.providers()
            .iter()
            .map(|p| ProviderInfo {
                id: p.id().to_string(),
                family: p.family(),
                name: p.display_name(),
                mock: p.is_mock(),
                models: p.models(),
                default_model: s.default_models.get(p.id()).cloned(),
                default_effort: s.default_efforts.get(p.id()).copied(),
                unsupported_models: s.unsupported_models.get(p.id()).cloned().unwrap_or_default(),
                health: health.get(p.id()).cloned(),
                quota: quotas.get(p.id()).cloned(),
            })
            .collect()
    }

    pub async fn refresh_health(&self) -> Vec<(String, Health)> {
        let mut set = JoinSet::new();
        for p in self.providers() {
            set.spawn(async move { (p.id().to_string(), p.health().await) });
        }
        let mut out = Vec::new();
        while let Some(Ok((id, h))) = set.join_next().await {
            self.health.write().unwrap().insert(id.clone(), h.clone());
            out.push((id, h));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub async fn refresh_quotas(&self) -> Vec<QuotaSnapshot> {
        let mut set = JoinSet::new();
        for p in self.providers() {
            set.spawn(async move { p.poll_quota().await });
        }
        while let Some(r) = set.join_next().await {
            if let Ok(Some(q)) = r {
                self.record_quota(q);
            }
        }
        self.quotas()
    }

    pub fn quota(&self, provider: &str) -> Option<QuotaSnapshot> {
        self.quotas.read().unwrap().get(provider).cloned()
    }

    pub fn quotas(&self) -> Vec<QuotaSnapshot> {
        let mut v: Vec<_> = self.quotas.read().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.provider.cmp(&b.provider));
        v
    }

    /// Merge a (possibly partial) snapshot, persist the cache and history, and broadcast it.
    pub fn record_quota(&self, snap: QuotaSnapshot) {
        let merged = {
            let mut q = self.quotas.write().unwrap();
            let merged = match q.get(&snap.provider) {
                Some(prev) => prev.merged_with(&snap),
                None => snap,
            };
            q.insert(merged.provider.clone(), merged.clone());
            merged
        };
        let all = self.quotas();
        let _ = self.store.save_quota_cache(&all);
        let sample = QuotaSample::from_snapshot(&merged);
        let store_it = {
            let mut last = self.last_samples.lock().unwrap();
            let ok = history::worth_storing(last.get(&merged.provider), &sample);
            if ok {
                last.insert(merged.provider.clone(), sample.clone());
            }
            ok
        };
        if store_it {
            let _ = history::append(&self.store.quota_history_path(), &sample);
        }
        self.emit(Event::Quota { snapshot: merged });
    }

    pub(crate) fn mark_unsupported(&self, provider: &str, model: &str) {
        let _ = self.update_settings(|s| {
            let list = s.unsupported_models.entry(provider.to_string()).or_default();
            if !list.iter().any(|m| m == model) {
                list.push(model.to_string());
            }
        });
    }

    /// Why `provider` must not take a new turn right now, if anything.
    pub(crate) fn quota_block(&self, provider: &str) -> Option<String> {
        self.quota(provider).and_then(|q| q.blocks(self.settings().quota_stop_percent, now_secs()))
    }

    /// Another ready provider to take a quota-blocked turn: same mock-ness, healthy (or not yet
    /// checked), not itself blocked; the other family first.
    pub(crate) fn reroute_candidate(&self, from: &Arc<dyn Provider>) -> Option<Arc<dyn Provider>> {
        let mut c: Vec<Arc<dyn Provider>> = self
            .providers()
            .into_iter()
            .filter(|p| p.id() != from.id() && p.is_mock() == from.is_mock())
            .filter(|p| self.health_of(p.id()).map_or(true, |h| h.ok))
            .filter(|p| self.quota_block(p.id()).is_none())
            .collect();
        c.sort_by_key(|p| p.family() == from.family());
        c.into_iter().next()
    }

    // ---------------------------------------------------------------- projects

    pub fn projects(&self) -> Vec<Project> {
        self.store.projects()
    }

    pub fn project(&self, id: &str) -> Option<Project> {
        self.projects().into_iter().find(|p| p.id == id)
    }

    pub fn create_project(&self, name: &str, path: &str) -> Result<Project> {
        let name = name.trim();
        let path = path.trim();
        if name.is_empty() {
            bail!("請輸入專案名稱");
        }
        if !std::path::Path::new(path).is_dir() {
            bail!("找不到資料夾：{path}");
        }
        let mut ps = self.projects();
        let p = Project { id: new_id("p"), name: name.into(), path: path.into(), created_at: now_ms(), memory: String::new() };
        ps.push(p.clone());
        self.store.save_projects(&ps)?;
        Ok(p)
    }

    pub fn update_project(&self, id: &str, name: Option<String>, path: Option<String>, memory: Option<String>) -> Result<Project> {
        let mut ps = self.projects();
        let p = ps.iter_mut().find(|p| p.id == id).ok_or_else(|| anyhow!("找不到專案"))?;
        if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            p.name = n;
        }
        if let Some(path) = path.map(|x| x.trim().to_string()) {
            if !std::path::Path::new(&path).is_dir() {
                bail!("找不到資料夾：{path}");
            }
            p.path = path;
        }
        if let Some(m) = memory {
            p.memory = m;
        }
        let out = p.clone();
        self.store.save_projects(&ps)?;
        Ok(out)
    }

    /// Append a finished session's verdict to its project's memory.
    /// Append one line to the project memory. `custom` is the user's (possibly edited)
    /// sentence: newlines removed, ≤ 300 chars, never empty; it keeps the `- [date] …【grade】`
    /// shape so the memory file stays uniform. `None` keeps the pre-v0.4 line.
    pub async fn remember(&self, session_id: &str, custom: Option<&str>) -> Result<Project> {
        let rec = self.session(session_id).await.ok_or_else(|| anyhow!("找不到議題"))?;
        let v = rec.verdict.as_ref().ok_or_else(|| anyhow!("這個議題還沒有結論"))?;
        let project = self.project(&rec.spec.project_id).ok_or_else(|| anyhow!("找不到專案"))?;
        if let Some(c) = custom {
            let one = c.split_whitespace().collect::<Vec<_>>().join(" ");
            if one.is_empty() {
                bail!("要存入記憶的句子是空的");
            }
            if one.chars().count() > 300 {
                bail!("要存入記憶的句子太長（上限 300 字）");
            }
            let line = format!("- [{}] {one}【{}】", format_date(rec.created_at), v.grade.label());
            let memory = if project.memory.trim().is_empty() { line } else { format!("{}\n{line}", project.memory.trim_end()) };
            return self.update_project(&project.id, None, None, Some(memory));
        }
        let line = format!(
            "- [{}] {}（{}）：{}【{}】",
            format_date(rec.created_at),
            rec.title,
            rec.spec.mode.label(),
            compact::truncate_chars(&v.headline, 120),
            v.grade.label()
        );
        let memory = if project.memory.trim().is_empty() { line } else { format!("{}\n{line}", project.memory.trim_end()) };
        self.update_project(&project.id, None, None, Some(memory))
    }

    // ---------------------------------------------------------------- sessions

    async fn all_records(&self) -> Vec<SessionRecord> {
        let mut by_id: HashMap<String, SessionRecord> = self.store.sessions().into_iter().map(|r| (r.id.clone(), r)).collect();
        for (id, ctl) in self.running.lock().await.iter() {
            by_id.insert(id.clone(), ctl.record.lock().await.clone());
        }
        let mut v: Vec<_> = by_id.into_values().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    /// Sessions of a project, optionally filtered by a case-insensitive text query over the
    /// title, prompt and every turn.
    pub async fn sessions(&self, project_id: Option<&str>, query: Option<&str>) -> Vec<SessionSummary> {
        let q = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());
        self.all_records()
            .await
            .iter()
            .filter(|r| project_id.map_or(true, |p| r.spec.project_id == p))
            .filter(|r| {
                q.as_ref().map_or(true, |q| {
                    r.title.to_lowercase().contains(q)
                        || r.spec.prompt.to_lowercase().contains(q)
                        || r.turns.iter().any(|t| t.text.to_lowercase().contains(q))
                })
            })
            .map(SessionSummary::from)
            .collect()
    }

    pub async fn session(&self, id: &str) -> Option<SessionRecord> {
        let ctl = self.running.lock().await.get(id).cloned();
        match ctl {
            Some(c) => Some(c.record.lock().await.clone()),
            None => self.store.load_session(id),
        }
    }

    pub async fn cancel(&self, id: &str) -> bool {
        if let Some(c) = self.running.lock().await.get(id) {
            c.cancel.cancel();
            return true;
        }
        // Waiting for approval after a restart: no runner exists, just close it.
        if let Some(mut r) = self.store.load_session(id).filter(|r| r.status == Status::AwaitingApproval) {
            r.status = Status::Cancelled;
            r.phase = None;
            r.updated_at = now_ms();
            let _ = self.store.save_session(&r);
            self.emit(Event::Status { session_id: r.id.clone(), title: r.title.clone(), status: r.status, error: None });
            return true;
        }
        false
    }

    pub async fn interject(&self, id: &str, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            bail!("插話內容是空的");
        }
        let ctl = self.running.lock().await.get(id).cloned().ok_or_else(|| anyhow!("議題已結束，無法插話"))?;
        let ij = Interjection { at: now_ms(), text: compact::truncate_chars(text, 4000), delivered: false };
        let snapshot = {
            let mut r = ctl.record.lock().await;
            r.interjections.push(ij.clone());
            r.updated_at = now_ms();
            r.clone()
        };
        let _ = self.store.save_session(&snapshot);
        self.emit(Event::Interjection { session_id: id.into(), interjection: ij });
        Ok(())
    }

    fn validate(&self, mut spec: SessionSpec) -> Result<(SessionSpec, Project, Vec<String>)> {
        let mut notes = Vec::new();
        spec.prompt = spec.prompt.trim().to_string();
        if spec.prompt.is_empty() {
            bail!("請輸入議題內容");
        }
        if spec.prompt.chars().count() > MAX_PROMPT_CHARS {
            bail!("議題內容太長（上限 {MAX_PROMPT_CHARS} 字）");
        }
        let project = self.project(&spec.project_id).ok_or_else(|| anyhow!("找不到專案"))?;
        if !std::path::Path::new(&project.path).is_dir() {
            bail!("專案資料夾不存在：{}", project.path);
        }
        spec.participants.retain(|p| !p.provider.trim().is_empty());
        for p in spec.participants.iter_mut().chain(spec.judge.iter_mut()).chain(spec.builder.iter_mut()).chain(spec.reviewer.iter_mut()) {
            if self.provider(&p.provider).is_none() {
                bail!("找不到模型提供者：{}", p.provider);
            }
            p.model = p.model.take().map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
            // An explicit effort must be one this provider/model lists — a 400, never a silent
            // downgrade (templates, schedules, experiments and MCP all come through here).
            if let (Some(e), Some(prov)) = (p.effort, self.provider(&p.provider)) {
                let ok = crate::provider::supported_efforts(&prov.models(), p.model.as_deref());
                if !ok.contains(&e) {
                    let which = p.model.clone().unwrap_or_else(|| "CLI 預設模型".into());
                    if ok.is_empty() {
                        bail!("{}（{which}）無法調整推理強度，請改回「模型預設」", prov.display_name());
                    }
                    bail!(
                        "{}（{which}）不支援推理強度「{}」；可用：{}",
                        prov.display_name(),
                        e.as_str(),
                        ok.iter().map(|x| x.as_str()).collect::<Vec<_>>().join("、")
                    );
                }
            }
        }
        let n = spec.participants.len();
        let same_family = |ps: &[Participant]| {
            let fams: Vec<Family> = ps.iter().filter_map(|p| self.provider(&p.provider).map(|x| x.family())).collect();
            fams.len() >= 2 && fams.iter().all(|f| *f == fams[0])
        };
        match spec.mode {
            Mode::Solo => {
                if n == 0 {
                    bail!("單人模式需要選一個模型");
                }
                spec.participants.truncate(1);
            }
            Mode::Relay | Mode::Debate => {
                if n == 0 {
                    bail!("{}模式需要至少一個模型", spec.mode.label());
                }
                spec.participants.truncate(2);
                if spec.participants.len() == 1 || same_family(&spec.participants) {
                    notes.push(match spec.mode {
                        Mode::Relay => "執行者與審查者是同一家模型：自我審查較容易漏掉同類盲點，建議改用另一家模型審查。".to_string(),
                        _ => "正反方是同一家模型：辯論的多樣性有限，建議正反方用不同模型。".to_string(),
                    });
                }
            }
            Mode::Roundtable => {
                if n < 2 {
                    bail!("討論模式需要至少兩位與談人");
                }
                spec.participants.truncate(4);
            }
            Mode::Workshop => {
                if n == 0 {
                    bail!("方案工作坊需要至少一位提案者");
                }
                spec.participants.truncate(4);
                if n == 1 || same_family(&spec.participants) {
                    notes.push("所有提案者都是同一家模型：方案的多樣性有限，建議 Claude 與 GPT 各提一份。".into());
                }
                if !spec.plan_only {
                    spec.access = Access::Write;
                }
            }
        }
        spec.verify_command = spec.verify_command.take().map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        if spec.verify_command.is_some() && matches!(spec.mode, Mode::Roundtable | Mode::Debate) {
            notes.push("討論／辯論模式不執行驗證指令，已忽略。".into());
            spec.verify_command = None;
        }
        if matches!(spec.mode, Mode::Roundtable | Mode::Debate) {
            spec.access = Access::ReadOnly;
        }
        Ok((spec, project, notes))
    }

    pub(crate) fn turn_budget(spec: &SessionSpec) -> u32 {
        let rounds = modes::rounds_for(spec);
        let n = spec.participants.len() as u32;
        let computed = match spec.mode {
            Mode::Solo => 1,
            Mode::Relay => 2 * rounds + 2,
            Mode::Roundtable => n * rounds + 1,
            Mode::Debate => 2 * rounds + 2,
            Mode::Workshop => {
                let planning = 2 * n + 1 + if spec.red_team { 2 } else { 0 } + MAX_PLAN_REVISIONS;
                planning + if spec.plan_only { 0 } else { 2 * rounds + 2 }
            }
        };
        spec.max_turns.map_or(computed, |m| m.max(1).min(computed)).min(HARD_TURN_CAP)
    }

    pub async fn start(self: &Arc<Self>, spec: SessionSpec) -> Result<SessionRecord> {
        let (spec, project, notes) = self.validate(spec)?;
        let title = spec
            .title
            .clone()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| prompts::headline(&spec.prompt, 40));
        let now = now_ms();

        // 分岔：copy the earlier transcript as seeded turns.
        let mut seeded = Vec::new();
        let mut fork_note = None;
        if let Some(seed) = &spec.seed {
            let src = self.session(&seed.session_id).await.ok_or_else(|| anyhow!("找不到要分岔的議題"))?;
            for t in src.turns.iter().filter(|t| t.index <= seed.upto && t.ended_at.is_some()) {
                let mut c = t.clone();
                c.index = seeded.len() as u32;
                c.seeded = true;
                c.delta_seq = 0;
                seeded.push(c);
            }
            fork_note = Some(format!("延續自議題「{}」的前 {} 則發言（分岔）。", src.title, seeded.len()));
        }
        let round_offset = seeded.iter().map(|t| t.round).max().unwrap_or(0);

        let max_turns = Self::turn_budget(&spec);
        let mut rec = SessionRecord {
            id: new_id("s"),
            spec: spec.clone(),
            title,
            status: Status::Running,
            created_at: now,
            updated_at: now,
            turns: seeded,
            notes: vec![],
            interjections: vec![],
            verify_runs: vec![],
            verdict: None,
            error: None,
            plan: None,
            workspace: None,
            phase: None,
        };
        rec.notes.push(Note { at: now, level: NoteLevel::Info, text: format!("本議題最多 {max_turns} 次模型發言（成本上限）。") });
        if let Some(n) = fork_note {
            rec.notes.push(Note { at: now, level: NoteLevel::Info, text: n });
        }
        for n in notes {
            rec.notes.push(Note { at: now, level: NoteLevel::Warn, text: n });
        }
        self.store.save_session(&rec)?;

        let snapshot = if self.settings().context_pack && matches!(spec.mode, Mode::Workshop | Mode::Roundtable | Mode::Debate) {
            Some(crate::repo::snapshot(std::path::Path::new(&project.path)).await)
        } else {
            None
        };
        let ctl = Arc::new(RunCtl::new(rec.clone()));
        self.running.lock().await.insert(rec.id.clone(), ctl.clone());
        let runner = Arc::new(Runner {
            engine: self.clone(),
            ctl,
            session_id: rec.id.clone(),
            workdir: std::sync::Mutex::new(project.path.clone().into()),
            project,
            spec,
            settings: self.settings(),
            turns_used: AtomicU32::new(0),
            max_turns,
            round_offset,
            snapshot,
            resume: std::sync::Mutex::new(None),
        });
        self.emit(Event::Status { session_id: rec.id.clone(), title: rec.title.clone(), status: Status::Running, error: None });
        tokio::spawn(runner.run());
        Ok(rec)
    }

    /// Spend a tiny amount of Claude quota to refresh the Claude meter (opt-in from the UI).
    pub async fn probe_claude(&self) -> Result<Option<QuotaSnapshot>> {
        let p = self.provider("claude").ok_or_else(|| anyhow!("找不到 Claude 提供者"))?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let req = TurnRequest {
            prompt: prompts::ask_probe(),
            model: Some("haiku".into()),
            cwd: self.store.sandbox_dir(),
            access: Access::ReadOnly,
            timeout: Duration::from_secs(120),
            cancel: CancellationToken::new(),
            effort: None,
            clean_config: self.settings().isolate_cli_config,
        };
        let handle = tokio::spawn(async move { p.run(req, tx).await });
        while let Some(ev) = rx.recv().await {
            if let ProviderEvent::Quota(q) = ev {
                self.record_quota(q);
            }
        }
        let out = handle.await.map_err(|e| anyhow!("{e}"))?;
        if !out.ok {
            bail!(out.error.unwrap_or_else(|| "探測失敗".into()));
        }
        Ok(self.quota("claude"))
    }
}

/// `YYYY-MM-DD` (UTC) from unix millis, without a date crate.
pub fn format_date(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dates() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(1_794_787_200_000), "2026-11-16");
    }

    #[test]
    fn gate_decisions_deserialize_from_the_ui_shape() {
        let a: GateDecision = serde_json::from_str(r#"{"action":"approve","plan":"x","verify_command":"cargo test"}"#).unwrap();
        assert!(matches!(a, GateDecision::Approve { plan: Some(_), verify_command: Some(_) }));
        let r: GateDecision = serde_json::from_str(r#"{"action":"revise","feedback":"更簡單"}"#).unwrap();
        assert!(matches!(r, GateDecision::Revise { .. }));
        let j: GateDecision = serde_json::from_str(r#"{"action":"reject"}"#).unwrap();
        assert!(matches!(j, GateDecision::Reject { reason: None }));
    }
}
